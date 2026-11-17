//! 配置文件读写：修订号、备份、testparm 校验、原子替换与 smbd 重载。

use std::fmt;
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use crate::conf::Doc;
use crate::util::{self, RunOutcome};

/// 语法校验器抽象，便于在测试中替换成确定性的实现。
pub trait Validator: Send + Sync {
    /// 校验候选配置。`Ok(Some(warning))` 表示通过但带提示，`Err(detail)` 表示失败。
    fn validate(&self, candidate: &Path) -> Result<Option<String>, String>;

    fn label(&self) -> String;
}

pub struct NoValidator;

impl Validator for NoValidator {
    fn validate(&self, _candidate: &Path) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn label(&self) -> String {
        "已禁用".to_string()
    }
}

pub struct Testparm {
    pub bin: PathBuf,
}

impl Validator for Testparm {
    fn validate(&self, candidate: &Path) -> Result<Option<String>, String> {
        let path = candidate
            .to_str()
            .ok_or_else(|| "临时文件路径不是有效的 UTF-8".to_string())?;
        let outcome = util::run_capture(
            "testparm",
            &self.bin,
            &["-s", path],
            Duration::from_secs(20),
        )
        .map_err(|err| format!("无法执行 testparm: {err}"))?;

        if outcome.timed_out {
            return Err("testparm 执行超时".to_string());
        }
        if outcome.exit_code == Some(0) {
            Ok(if outcome.output.trim().is_empty() {
                None
            } else {
                Some(outcome.output)
            })
        } else if outcome.output.trim().is_empty() {
            Err(format!("testparm 校验失败（{}）", outcome.summary()))
        } else {
            Err(outcome.output)
        }
    }

    fn label(&self) -> String {
        format!("{} -s <候选文件>", self.bin.display())
    }
}

#[derive(Debug)]
pub struct Loaded {
    pub doc: Doc,
    pub text: String,
    pub rev: String,
    pub writable: bool,
    pub size: u64,
}

#[derive(Debug)]
pub struct CommitInfo {
    pub backup: Option<PathBuf>,
    pub reload: Option<RunOutcome>,
    pub warnings: Option<String>,
}

#[derive(Debug)]
pub enum CommitError {
    Conflict,
    Io(String),
    Validate(String),
}

impl fmt::Display for CommitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommitError::Conflict => write!(f, "配置文件已被其他程序修改"),
            CommitError::Io(msg) => write!(f, "{msg}"),
            CommitError::Validate(msg) => write!(f, "{msg}"),
        }
    }
}

pub trait Reloader: Send + Sync {
    /// `None` 表示未配置重载；`Some` 表示已执行（含失败结果）。
    fn reload(&self) -> Option<RunOutcome>;

    fn label(&self) -> String;
}

pub struct NoReloader;

impl Reloader for NoReloader {
    fn reload(&self) -> Option<RunOutcome> {
        None
    }

    fn label(&self) -> String {
        "已禁用".to_string()
    }
}

pub struct ShellReloader {
    pub command: String,
}

impl Reloader for ShellReloader {
    fn reload(&self) -> Option<RunOutcome> {
        Some(
            match util::run_shell("reload", &self.command, Duration::from_secs(15)) {
                Ok(outcome) => outcome,
                Err(message) => RunOutcome {
                    exit_code: None,
                    timed_out: false,
                    output: message,
                },
            },
        )
    }

    fn label(&self) -> String {
        self.command.clone()
    }
}

pub struct Store {
    pub conf: PathBuf,
    pub backup_dir: PathBuf,
    pub max_backups: usize,
    pub validator: Box<dyn Validator>,
    pub reload: Box<dyn Reloader>,
}

impl Store {
    pub fn new(
        conf: PathBuf,
        backup_dir: PathBuf,
        max_backups: usize,
        validator: Box<dyn Validator>,
        reload: Box<dyn Reloader>,
    ) -> Store {
        Store {
            conf,
            backup_dir,
            max_backups,
            validator,
            reload,
        }
    }

    pub fn load(&self) -> Result<Loaded, String> {
        let bytes = fs::read(&self.conf)
            .map_err(|err| format!("无法读取 {}: {err}", self.conf.display()))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| "配置文件不是有效的 UTF-8 文本，已拒绝编辑".to_string())?;
        let meta = fs::metadata(&self.conf)
            .map_err(|err| format!("无法读取文件属性 {}: {err}", self.conf.display()))?;
        Ok(Loaded {
            doc: Doc::parse(&text),
            text,
            rev: revision(&meta),
            writable: !meta.permissions().readonly(),
            size: meta.len(),
        })
    }

    pub fn commit(&self, doc: &Doc, expected_rev: &str) -> Result<CommitInfo, CommitError> {
        let meta = fs::metadata(&self.conf)
            .map_err(|err| CommitError::Io(format!("无法读取配置文件属性: {err}")))?;
        if revision(&meta) != expected_rev {
            return Err(CommitError::Conflict);
        }

        let text = doc.render();
        let parent = self.conf.parent().unwrap_or_else(|| Path::new("."));
        let file_name = self
            .conf
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("smb.conf");
        let temp =
            write_temp(parent, file_name, &text, unix_mode(&meta)).map_err(CommitError::Io)?;

        let warnings = match self.validator.validate(&temp) {
            Ok(warnings) => warnings,
            Err(detail) => {
                let _ = fs::remove_file(&temp);
                return Err(CommitError::Validate(detail));
            }
        };

        let backup = if self.max_backups > 0 {
            match self.make_backup(file_name) {
                Ok(path) => Some(path),
                Err(err) => {
                    let _ = fs::remove_file(&temp);
                    return Err(CommitError::Io(err));
                }
            }
        } else {
            None
        };

        if let Err(err) = replace_file(&temp, &self.conf) {
            let _ = fs::remove_file(&temp);
            return Err(CommitError::Io(format!("写入配置失败: {err}")));
        }
        sync_dir(parent);

        let reload = self.reload.reload();

        Ok(CommitInfo {
            backup,
            reload,
            warnings,
        })
    }

    pub fn validator_label(&self) -> String {
        self.validator.label()
    }

    pub fn reload_label(&self) -> String {
        self.reload.label()
    }

    pub fn backup_count(&self) -> usize {
        let prefix = match self.conf.file_name().and_then(|name| name.to_str()) {
            Some(name) => format!("{name}."),
            None => return 0,
        };
        fs::read_dir(&self.backup_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| {
                        entry
                            .file_name()
                            .to_str()
                            .map(|name| name.starts_with(&prefix) && name.ends_with(".bak"))
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    fn make_backup(&self, file_name: &str) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.backup_dir)
            .map_err(|err| format!("无法创建备份目录 {}: {err}", self.backup_dir.display()))?;

        let stamp = util::compact_time(util::now_unix());
        let mut counter = 0;
        let mut target = self
            .backup_dir
            .join(format!("{file_name}.{stamp}-{counter:03}.bak"));
        while target.exists() {
            counter += 1;
            if counter > 10_000 {
                return Err("备份文件名冲突过多".to_string());
            }
            target = self
                .backup_dir
                .join(format!("{file_name}.{stamp}-{counter:03}.bak"));
        }

        fs::copy(&self.conf, &target).map_err(|err| format!("备份失败: {err}"))?;
        self.prune_backups(file_name);
        Ok(target)
    }

    fn prune_backups(&self, file_name: &str) {
        let prefix = format!("{file_name}.");
        let mut backups: Vec<PathBuf> = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.backup_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    if name.starts_with(&prefix) && name.ends_with(".bak") {
                        backups.push(path);
                    }
                }
            }
        }
        backups.sort();
        while backups.len() > self.max_backups {
            let oldest = backups.remove(0);
            let _ = fs::remove_file(oldest);
        }
    }
}

fn revision(meta: &fs::Metadata) -> String {
    let nanos = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{nanos}-{}", meta.len())
}

fn write_temp(
    parent: &Path,
    file_name: &str,
    text: &str,
    mode: Option<u32>,
) -> Result<PathBuf, String> {
    for attempt in 0..16 {
        let path = parent.join(format!(
            ".{file_name}.samba-ui-{}-{attempt}.tmp",
            std::process::id()
        ));
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(format!(
                    "无法创建临时文件 {}: {err}（请确认进程对配置目录有写权限）",
                    path.display()
                ))
            }
        };
        if let Err(err) = file
            .write_all(text.as_bytes())
            .and_then(|_| file.sync_all())
        {
            let _ = fs::remove_file(&path);
            return Err(format!("写入临时文件失败: {err}"));
        }
        drop(file);

        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            if let Err(err) = fs::set_permissions(&path, fs::Permissions::from_mode(mode & 0o777)) {
                let _ = fs::remove_file(&path);
                return Err(format!("设置临时文件权限失败: {err}"));
            }
        }
        #[cfg(not(unix))]
        let _ = mode;

        return Ok(path);
    }
    Err("无法创建临时文件：重名过多".to_string())
}

#[cfg(unix)]
fn unix_mode(meta: &fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(meta.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn unix_mode(_meta: &fs::Metadata) -> Option<u32> {
    None
}

#[cfg(unix)]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    if target.exists() {
        fs::remove_file(target)?;
    }
    fs::rename(source, target)
}

fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    {
        if let Ok(handle) = File::open(dir) {
            let _ = handle.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let name = format!(
                "samba-ui-store-{tag}-{}-{}",
                std::process::id(),
                util::next_id()
            );
            let mut candidates: Vec<PathBuf> = Vec::new();
            if let Some(base) = std::env::var_os("SAMBA_UI_TEST_TMP") {
                candidates.push(PathBuf::from(base));
            }
            candidates.push(std::env::temp_dir());
            if let Ok(cwd) = std::env::current_dir() {
                candidates.push(cwd.join("target").join("test-tmp"));
            }
            for base in candidates {
                let path = base.join(&name);
                if fs::create_dir_all(&path).is_ok() {
                    return TempDir { path };
                }
            }
            panic!("无法创建测试临时目录（可用 SAMBA_UI_TEST_TMP 指定）");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    struct FailValidator;

    impl Validator for FailValidator {
        fn validate(&self, _candidate: &Path) -> Result<Option<String>, String> {
            Err("模拟校验失败".to_string())
        }

        fn label(&self) -> String {
            "fail".to_string()
        }
    }

    struct FailReloader;

    impl Reloader for FailReloader {
        fn reload(&self) -> Option<RunOutcome> {
            Some(RunOutcome {
                exit_code: Some(1),
                timed_out: false,
                output: "reload failed".to_string(),
            })
        }

        fn label(&self) -> String {
            "fail-reload".to_string()
        }
    }

    fn store(dir: &TempDir) -> Store {
        Store::new(
            dir.path.join("smb.conf"),
            dir.path.join("backups"),
            3,
            Box::new(NoValidator),
            Box::new(NoReloader),
        )
    }

    fn write_sample(dir: &TempDir) {
        fs::write(
            dir.path.join("smb.conf"),
            "[global]\n   workgroup = A\n\n[share]\n   path = /srv/share\n",
        )
        .unwrap();
    }

    #[test]
    fn load_and_commit_roundtrip() {
        let dir = TempDir::new("roundtrip");
        write_sample(&dir);
        let store = store(&dir);

        let loaded = store.load().unwrap();
        let mut doc = loaded.doc.clone();
        doc.apply_edits(
            "global",
            &[crate::conf::Edit::Set {
                key: "workgroup".to_string(),
                value: "B".to_string(),
            }],
        )
        .unwrap();

        let info = store.commit(&doc, &loaded.rev).unwrap();
        assert!(info.backup.is_some());
        assert!(info.reload.is_none());
        let backup = info.backup.unwrap();
        assert!(backup.exists());
        assert_eq!(
            fs::read_to_string(dir.path.join("smb.conf")).unwrap(),
            "[global]\n   workgroup = B\n\n[share]\n   path = /srv/share\n"
        );

        let loaded = store.load().unwrap();
        assert_eq!(loaded.doc.get("global", "workgroup").as_deref(), Some("B"));
    }

    #[test]
    fn stale_revision_is_rejected() {
        let dir = TempDir::new("conflict");
        write_sample(&dir);
        let store = store(&dir);
        let loaded = store.load().unwrap();

        fs::write(dir.path.join("smb.conf"), "[global]\n   workgroup = X\n").unwrap();
        let mut doc = loaded.doc.clone();
        doc.apply_edits(
            "global",
            &[crate::conf::Edit::Set {
                key: "workgroup".to_string(),
                value: "B".to_string(),
            }],
        )
        .unwrap();
        assert!(matches!(
            store.commit(&doc, &loaded.rev),
            Err(CommitError::Conflict)
        ));
    }

    #[test]
    fn validator_failure_keeps_original() {
        let dir = TempDir::new("validate");
        write_sample(&dir);
        let store = Store::new(
            dir.path.join("smb.conf"),
            dir.path.join("backups"),
            3,
            Box::new(FailValidator),
            Box::new(NoReloader),
        );
        let loaded = store.load().unwrap();
        let mut doc = loaded.doc.clone();
        doc.apply_edits(
            "global",
            &[crate::conf::Edit::Set {
                key: "workgroup".to_string(),
                value: "B".to_string(),
            }],
        )
        .unwrap();

        match store.commit(&doc, &loaded.rev) {
            Err(CommitError::Validate(msg)) => assert!(msg.contains("模拟校验失败")),
            other => panic!("expected validate error, got {other:?}"),
        }
        assert_eq!(
            fs::read_to_string(dir.path.join("smb.conf")).unwrap(),
            "[global]\n   workgroup = A\n\n[share]\n   path = /srv/share\n"
        );
        let leftovers: Vec<_> = fs::read_dir(&dir.path)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "临时文件未清理: {leftovers:?}");
    }

    #[test]
    fn reload_command_outcome_is_reported() {
        let dir = TempDir::new("reload");
        write_sample(&dir);
        let store = Store::new(
            dir.path.join("smb.conf"),
            dir.path.join("backups"),
            3,
            Box::new(NoValidator),
            Box::new(FailReloader),
        );
        let loaded = store.load().unwrap();
        let info = store
            .commit(&loaded.doc, &loaded.rev)
            .expect("commit should succeed");
        let reload = info.reload.expect("reload outcome");
        assert!(!reload.success(), "expected failed reload: {reload:?}");
        assert!(reload.output.contains("reload failed"));
    }

    #[test]
    fn backup_count_is_pruned() {
        let dir = TempDir::new("prune");
        write_sample(&dir);
        let store = store(&dir);
        for index in 0..6 {
            let loaded = store.load().unwrap();
            let mut doc = loaded.doc.clone();
            doc.apply_edits(
                "global",
                &[crate::conf::Edit::Set {
                    key: "workgroup".to_string(),
                    value: format!("G{index}"),
                }],
            )
            .unwrap();
            store.commit(&doc, &loaded.rev).unwrap();
            std::thread::sleep(Duration::from_millis(15));
        }
        assert_eq!(store.backup_count(), 3);
    }

    #[test]
    fn missing_file_reports_clear_error() {
        let dir = TempDir::new("missing");
        let store = store(&dir);
        let err = store.load().unwrap_err();
        assert!(err.contains("无法读取"), "{err}");
    }
}
