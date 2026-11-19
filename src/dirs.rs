//! 扫描常用目录和挂载点，作为新建共享时的路径候选。

use std::fs;
use std::path::{Path, PathBuf};

const MAX_CANDIDATES: usize = 100;

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
    let mut roots: Vec<PathBuf> = vec![PathBuf::from("/")];
    let mut fixed = Vec::new();

    for directory in FIXED_DIRS {
        let path = PathBuf::from(directory);
        if path.is_dir() {
            fixed.push(directory.to_string());
            roots.push(path);
        }
    }

    let mut result = from_roots(&roots);
    result.extend(fixed);
    normalize(result)
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
