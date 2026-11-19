//! 扫描常用目录和挂载点，作为新建共享时的路径候选。

use std::fs;
use std::path::{Path, PathBuf};

const MAX_CANDIDATES: usize = 100;
const MAX_BROWSE_ENTRIES: usize = 500;

const FORBIDDEN_DIRS: &[&str] = &[
    "/bin", "/boot", "/dev", "/etc", "/lib", "/lib32", "/lib64", "/libx32", "/proc", "/root",
    "/run", "/sbin", "/sys", "/usr", "/var",
];

const FIXED_DIRS: &[&str] = &[
    "/srv",
    "/srv/samba",
    "/mnt",
    "/media",
    "/media/pi",
    "/home",
    "/data",
];

const SKIP_ROOT: &[&str] = &[
    "bin",
    "boot",
    "dev",
    "etc",
    "lib",
    "lib32",
    "lib64",
    "libx32",
    "lost+found",
    "proc",
    "run",
    "sbin",
    "sys",
    "tmp",
    "usr",
    "var",
];

/// 返回一组可直接用于 `path` 输入框的目录，最多 100 项。
pub fn candidates() -> Vec<String> {
    let roots = roots();
    let mut result = from_roots(&roots);
    result.extend(roots.iter().map(|path| path.display().to_string()));
    normalize(result)
}

/// 可供目录浏览器选择的起始目录。
pub fn roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for directory in FIXED_DIRS {
        let path = PathBuf::from(directory);
        if path.is_dir() && !is_forbidden(&path) {
            roots.push(path);
        }
    }
    if let Ok(entries) = fs::read_dir("/") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if SKIP_ROOT.contains(&name.as_ref()) {
                continue;
            }
            let path = entry.path();
            if path.is_dir() && !is_forbidden(&path) {
                roots.push(path);
            }
        }
    }
    normalize_paths(roots)
}

fn normalize_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let strings: Vec<String> = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    normalize(strings).into_iter().map(PathBuf::from).collect()
}

/// 规范化并校验一个浏览路径，拒绝系统目录和非目录路径。
pub fn canonical_browse_path(raw: &str) -> Result<PathBuf, String> {
    if raw.is_empty() || raw.contains('\0') {
        return Err("目录路径不能为空".to_string());
    }
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err("目录路径必须是绝对路径".to_string());
    }
    let canonical =
        fs::canonicalize(&path).map_err(|err| format!("无法打开目录 {}: {err}", path.display()))?;
    if !canonical.is_dir() {
        return Err(format!("{} 不是目录", canonical.display()));
    }
    if canonical == Path::new("/") || is_forbidden(&canonical) {
        return Err("不能浏览系统目录，请直接输入路径".to_string());
    }
    Ok(canonical)
}

pub fn is_forbidden(path: &Path) -> bool {
    FORBIDDEN_DIRS.iter().any(|prefix| {
        let prefix = Path::new(prefix);
        path == prefix || path.starts_with(prefix)
    })
}

/// 列出目录的直接子目录，最多 500 项。
pub fn list_dir(path: &Path) -> Result<Vec<(String, String)>, String> {
    let entries =
        fs::read_dir(path).map_err(|err| format!("无法读取目录 {}: {err}", path.display()))?;
    let mut directories = Vec::new();
    for entry in entries.flatten() {
        let child = entry.path();
        if !child.is_dir() || is_forbidden(&child) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.is_empty() {
            continue;
        }
        directories.push((name, child.display().to_string()));
        if directories.len() >= MAX_BROWSE_ENTRIES {
            break;
        }
    }
    directories.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(directories)
}

/// 扫描每个根目录的直接子目录。单独拆出来便于测试。
pub fn from_roots(roots: &[PathBuf]) -> Vec<String> {
    let mut result = Vec::new();

    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if root == Path::new("/") {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if SKIP_ROOT.contains(&name.as_ref()) {
                    continue;
                }
            }
            result.push(path.display().to_string());
            if result.len() >= MAX_CANDIDATES {
                break;
            }
        }
        if result.len() >= MAX_CANDIDATES {
            break;
        }
    }

    normalize(result)
}

fn normalize(mut directories: Vec<String>) -> Vec<String> {
    directories.retain(|directory| !directory.is_empty());
    directories.sort();
    directories.dedup();
    directories.truncate(MAX_CANDIDATES);
    directories
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn lists_only_directories_and_dedupes() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("dirs-{}-{stamp}", std::process::id()));
        let alpha = base.join("alpha");
        let beta = base.join("beta");
        fs::create_dir_all(alpha).unwrap();
        fs::create_dir_all(beta).unwrap();
        fs::write(base.join("file.txt"), "x").unwrap();

        let directories = from_roots(&[base.clone()]);
        assert_eq!(directories.len(), 2);
        assert!(directories.iter().any(|path| path.ends_with("alpha")));
        assert!(directories.iter().any(|path| path.ends_with("beta")));
        assert!(!directories.iter().any(|path| path.ends_with("file.txt")));

        let again = from_roots(&[base.clone(), base.clone()]);
        assert_eq!(again.len(), 2);

        let _ = fs::remove_dir_all(&base);
    }
}
