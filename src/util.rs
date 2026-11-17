//! 与业务无关的小工具：时间格式化、日志、子进程执行、CSRF 令牌等。

use std::collections::hash_map::RandomState;
use std::fs::File;
use std::hash::{BuildHasher, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn next_id() -> u64 {
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn now_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// 把 Unix 秒数转换为 UTC 日期时间分量：(年, 月, 日, 时, 分, 秒)。
pub fn utc_parts(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    (
        year,
        month,
        day,
        (rem / 3_600) as u32,
        ((rem % 3_600) / 60) as u32,
        (rem % 60) as u32,
    )
}

/// Howard Hinnant 的 civil_from_days 算法（无时区数据库依赖）。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

pub fn utc_string(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = utc_parts(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// 用于备份文件名的紧凑时间戳，例如 `20261116-172417`。
pub fn compact_time(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = utc_parts(secs);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}")
}

/// RFC 7231 IMF-fixdate，例如 `Thu, 01 Jan 1970 00:00:00 GMT`。
pub fn http_date(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = utc_parts(secs);
    let wd = ((secs / 86_400 + 4) % 7) as usize;
    format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        WEEKDAYS[wd],
        d,
        MONTHS[(mo as usize - 1).min(11)],
        y,
        h,
        mi,
        s
    )
}

pub fn log(level: &str, msg: impl AsRef<str>) {
    eprintln!("{} [{}] {}", utc_string(now_unix()), level, msg.as_ref());
}

pub fn info(msg: impl AsRef<str>) {
    log("INFO", msg);
}

pub fn warn(msg: impl AsRef<str>) {
    log("WARN", msg);
}

pub fn error(msg: impl AsRef<str>) {
    log("ERROR", msg);
}

/// 在 PATH 中查找可执行文件；Windows 下自动尝试 `.exe` 后缀。
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

/// 生成 32 个十六进制字符的 CSRF 令牌。
pub fn csrf_token() -> String {
    let nanos = now_unix_nanos();
    let pid = std::process::id();

    let mut first = RandomState::new().build_hasher();
    first.write_u64(nanos as u64);
    first.write_u64((nanos >> 64) as u64);
    first.write_u32(pid);
    first.write_usize(&first as *const _ as usize);
    let a = first.finish();

    let mut second = RandomState::new().build_hasher();
    second.write_u64(a);
    second.write_u64(nanos as u64);
    second.write_u32(pid);
    second.write_usize(&second as *const _ as usize);
    let b = second.finish();

    format!("{a:016x}{b:016x}")
}

/// 常量时间比较，降低基于计时差异猜测口令的风险。
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[derive(Clone, Debug)]
pub struct RunOutcome {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub output: String,
}

impl RunOutcome {
    pub fn success(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }

    pub fn summary(&self) -> String {
        if self.timed_out {
            "执行超时".to_string()
        } else {
            match self.exit_code {
                Some(0) => "成功".to_string(),
                Some(code) => format!("退出码 {code}"),
                None => "被信号终止".to_string(),
            }
        }
    }
}

pub fn run_capture(
    label: &str,
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<RunOutcome, String> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    run_command(label, cmd, timeout)
}

pub fn run_shell(label: &str, script: &str, timeout: Duration) -> Result<RunOutcome, String> {
    #[cfg(unix)]
    let cmd = {
        let mut c = Command::new("sh");
        c.arg("-c").arg(script);
        c
    };
    #[cfg(windows)]
    let cmd = {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(script);
        c
    };
    run_command(label, cmd, timeout)
}

/// 运行子进程并将 stdout/stderr 重定向到临时文件，以支持超时控制。
///
/// 不直接使用管道，避免在受限环境下管道不可用或子进程阻塞。
fn run_command(label: &str, mut cmd: Command, timeout: Duration) -> Result<RunOutcome, String> {
    let id = next_id();
    let dir = std::env::temp_dir();
    let out_path = dir.join(format!("samba-ui-{}-{id}.out", std::process::id()));
    let err_path = dir.join(format!("samba-ui-{}-{id}.err", std::process::id()));

    let result = (|| -> Result<RunOutcome, String> {
        let out = File::create(&out_path).map_err(|e| format!("无法创建临时输出文件: {e}"))?;
        let err = File::create(&err_path).map_err(|e| format!("无法创建临时输出文件: {e}"))?;
        cmd.stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err));

        let mut child = cmd.spawn().map_err(|e| format!("无法启动 {label}: {e}"))?;

        let started = Instant::now();
        let mut timed_out = false;
        let exit_code = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.code(),
                Ok(None) => {
                    if started.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        timed_out = true;
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => {
                    let _ = child.kill();
                    return Err(format!("等待 {label} 失败: {e}"));
                }
            }
        };

        let mut output = String::new();
        let err_text = read_limited(&err_path, 32 * 1024);
        let out_text = read_limited(&out_path, 32 * 1024);
        if !err_text.trim().is_empty() {
            output.push_str(err_text.trim_end());
        }
        if !out_text.trim().is_empty() {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(out_text.trim_end());
        }
        if output.chars().count() > 8_000 {
            output = output.chars().take(8_000).collect();
            output.push_str("\n…（输出已截断）");
        }

        Ok(RunOutcome {
            exit_code,
            timed_out,
            output,
        })
    })();

    let _ = std::fs::remove_file(&out_path);
    let _ = std::fs::remove_file(&err_path);
    result
}

fn read_limited(path: &Path, limit: u64) -> String {
    let mut buf = Vec::new();
    if let Ok(file) = File::open(path) {
        let _ = file.take(limit).read_to_end(&mut buf);
    }
    String::from_utf8_lossy(&buf).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_epoch() {
        assert_eq!(utc_string(0), "1970-01-01T00:00:00Z");
        assert_eq!(http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
    }

    #[test]
    fn known_timestamps() {
        // 2023-11-14 22:13:20 UTC
        assert_eq!(utc_string(1_700_000_000), "2023-11-14T22:13:20Z");
        // 2024-02-29 00:00:00 UTC（闰年）
        assert_eq!(utc_string(1_709_164_800), "2024-02-29T00:00:00Z");
        assert_eq!(compact_time(1_700_000_000), "20231114-221320");
    }

    #[test]
    fn http_date_weekday() {
        // 2026-11-16 是星期一
        assert_eq!(http_date(1_794_787_200), "Mon, 16 Nov 2026 00:00:00 GMT");
    }

    #[test]
    fn constant_time_compare() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"ab"));
    }

    #[test]
    fn csrf_tokens_are_hex_and_unique() {
        let a = csrf_token();
        let b = csrf_token();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[cfg(unix)]
    #[test]
    fn shell_capture_works() {
        let outcome = run_shell("echo", "printf hello", Duration::from_secs(5)).unwrap();
        assert!(outcome.success());
        assert_eq!(outcome.output, "hello");
    }
}
