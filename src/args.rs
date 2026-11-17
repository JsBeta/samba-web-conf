//! 命令行参数与环境变量解析（不使用 clap，保持零依赖）。

use std::net::IpAddr;
use std::path::PathBuf;

use crate::VERSION;

#[derive(Clone, Debug)]
pub struct Options {
    pub bind: IpAddr,
    pub port: u16,
    pub conf: PathBuf,
    pub user: Option<String>,
    pub password: Option<String>,
    pub reload_cmd: Option<String>,
    pub testparm: Option<PathBuf>,
    pub backup_dir: PathBuf,
    pub max_backups: usize,
    pub allow_insecure: bool,
    pub read_only: bool,
    pub allow_origins: Vec<String>,
}

#[allow(clippy::large_enum_variant)] // 启动时只构造一次，Options 直接内联即可。
#[derive(Debug)]
pub enum Parsed {
    Run(Options),
    Help,
    Version,
}

pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Parsed, String> {
    parse_args_with_env(args, |key| std::env::var(key).ok())
}

fn next_value(
    args: &[String],
    index: &mut usize,
    inline: &Option<String>,
    name: &str,
) -> Result<String, String> {
    if let Some(value) = inline {
        return Ok(value.clone());
    }
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("参数 {name} 需要一个值"))
}

fn split_origins(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|origin| origin.trim().trim_end_matches('/'))
        .filter(|origin| !origin.is_empty())
        .map(|origin| origin.to_string())
        .collect()
}

pub fn parse_args_with_env<I, F>(args: I, env: F) -> Result<Parsed, String>
where
    I: IntoIterator<Item = String>,
    F: Fn(&str) -> Option<String>,
{
    let mut bind = env("SAMBA_UI_BIND").unwrap_or_else(|| "127.0.0.1".to_string());
    let mut port: u16 = match env("SAMBA_UI_PORT") {
        Some(value) => value
            .parse()
            .map_err(|_| format!("环境变量 SAMBA_UI_PORT 不是合法端口: {value}"))?,
        None => 8080,
    };
    let mut conf =
        PathBuf::from(env("SAMBA_UI_CONF").unwrap_or_else(|| "/etc/samba/smb.conf".to_string()));
    let mut user = env("SAMBA_UI_USER");
    let mut password = env("SAMBA_UI_PASSWORD");
    let mut reload_cmd =
        env("SAMBA_UI_RELOAD").or_else(|| Some("smbcontrol all reload-config".to_string()));
    let mut testparm = env("SAMBA_UI_TESTPARM")
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from("testparm")));
    let mut backup_dir = env("SAMBA_UI_BACKUP_DIR").map(PathBuf::from);
    let mut max_backups: usize = match env("SAMBA_UI_BACKUPS") {
        Some(value) => value
            .parse()
            .map_err(|_| format!("环境变量 SAMBA_UI_BACKUPS 不是合法数字: {value}"))?,
        None => 10,
    };
    let mut allow_insecure = false;
    let mut read_only = false;
    let mut no_auth = false;
    let mut allow_origins = env("SAMBA_UI_ALLOW_ORIGINS")
        .map(|value| split_origins(&value))
        .unwrap_or_default();

    let args: Vec<String> = args.into_iter().collect();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value.to_string())),
            None => (arg.as_str(), None),
        };
        match name {
            "--help" | "-h" => return Ok(Parsed::Help),
            "--version" | "-V" => return Ok(Parsed::Version),
            "--bind" => bind = next_value(&args, &mut index, &inline, name)?,
            "--port" => {
                let value = next_value(&args, &mut index, &inline, name)?;
                port = value
                    .parse()
                    .map_err(|_| format!("端口不是合法数字: {value}"))?;
            }
            "--conf" => conf = PathBuf::from(next_value(&args, &mut index, &inline, name)?),
            "--user" => user = Some(next_value(&args, &mut index, &inline, name)?),
            "--password" => password = Some(next_value(&args, &mut index, &inline, name)?),
            "--reload" => {
                let value = next_value(&args, &mut index, &inline, name)?;
                reload_cmd = if value.trim().is_empty() {
                    None
                } else {
                    Some(value)
                };
            }
            "--no-reload" => reload_cmd = None,
            "--testparm" => {
                let value = next_value(&args, &mut index, &inline, name)?;
                testparm = if value.trim().is_empty() {
                    None
                } else {
                    Some(PathBuf::from(value))
                };
            }
            "--no-testparm" => testparm = None,
            "--backup-dir" => {
                backup_dir = Some(PathBuf::from(next_value(&args, &mut index, &inline, name)?))
            }
            "--backups" => {
                let value = next_value(&args, &mut index, &inline, name)?;
                max_backups = value
                    .parse()
                    .map_err(|_| format!("备份数量不是合法数字: {value}"))?;
            }
            "--read-only" => read_only = true,
            "--allow-insecure" => allow_insecure = true,
            "--allow-origin" => {
                let value = next_value(&args, &mut index, &inline, name)?;
                allow_origins.extend(split_origins(&value));
            }
            "--no-auth" => no_auth = true,
            other => {
                return Err(format!("未知参数 {other}，使用 --help 查看用法"));
            }
        }
        index += 1;
    }

    if no_auth {
        user = None;
        password = None;
    }
    if user.is_some() != password.is_some() {
        return Err("--user 与 --password 必须同时提供".to_string());
    }
    if let Some(value) = &password {
        if value.is_empty() {
            return Err("认证口令不能为空".to_string());
        }
    }
    if conf.as_os_str().is_empty() {
        return Err("--conf 不能为空".to_string());
    }
    if port == 0 {
        return Err("端口不能为 0".to_string());
    }

    if bind.eq_ignore_ascii_case("localhost") {
        bind = "127.0.0.1".to_string();
    }
    let bind: IpAddr = bind
        .parse()
        .map_err(|_| format!("--bind 只支持 IP 地址或 localhost: {bind}"))?;

    let auth_enabled = user.is_some();
    if !auth_enabled && !bind.is_loopback() && !allow_insecure {
        return Err(format!(
            "拒绝在非回环地址 {bind} 上无认证运行。请加 --user/--password，\
             或明确加 --allow-insecure（不推荐）"
        ));
    }

    let backup_dir = backup_dir.unwrap_or_else(|| {
        conf.parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("samba-ui-backups")
    });

    Ok(Parsed::Run(Options {
        bind,
        port,
        conf,
        user,
        password,
        reload_cmd,
        testparm,
        backup_dir,
        max_backups,
        allow_insecure,
        read_only,
        allow_origins,
    }))
}

pub fn help_text() -> String {
    format!(
        "samba-ui {VERSION} — 零依赖的 Samba smb.conf 可视化管理页面

用法: samba-ui [选项]

  --bind <IP>        监听地址（默认 127.0.0.1；仅支持 IP 或 localhost）
  --port <PORT>      监听端口（默认 8080）
  --conf <PATH>      smb.conf 路径（默认 /etc/samba/smb.conf）
  --user <NAME>      启用 HTTP Basic 认证的用户名
  --password <PASS>  Basic 认证口令（建议改用 SAMBA_UI_PASSWORD 环境变量）
  --reload <CMD>     保存后执行的重载命令（默认 smbcontrol all reload-config；空字符串禁用）
  --no-reload        禁用保存后的自动重载
  --testparm <PATH>  语法校验程序（默认 testparm；空字符串禁用）
  --no-testparm      禁用语法校验
  --backup-dir <DIR> 备份目录（默认 <smb.conf 目录>/samba-ui-backups）
  --backups <N>      保留备份数量（默认 10；0 表示不备份）
  --read-only        只读模式：可以查看，禁止任何保存
  --allow-insecure   允许无认证绑定非回环地址（不推荐）
  --allow-origin <ORIGIN>
                     允许额外的来源地址，例如 http://nas:8080（可重复或用逗号分隔）
  --no-auth          强制关闭认证（仅本地调试）
  -h, --help         显示本帮助
  -V, --version      显示版本

环境变量:
  SAMBA_UI_BIND / SAMBA_UI_PORT / SAMBA_UI_CONF / SAMBA_UI_USER /
  SAMBA_UI_PASSWORD / SAMBA_UI_RELOAD / SAMBA_UI_TESTPARM /
  SAMBA_UI_BACKUP_DIR / SAMBA_UI_BACKUPS / SAMBA_UI_ALLOW_ORIGINS

示例:
  sudo samba-ui
  sudo samba-ui --conf /etc/samba/smb.conf --port 8080
  sudo SAMBA_UI_PASSWORD='s3cret' samba-ui --bind 0.0.0.0 --user admin

安全提示:
  页面使用明文 HTTP。远程访问请用 SSH 隧道或反向代理加 TLS；
  修改 smb.conf 通常需要 root 权限，建议以 root 运行并只监听 127.0.0.1。
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str], env: &[(&str, &str)]) -> Result<Parsed, String> {
        let env_map: Vec<(String, String)> = env
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        parse_args_with_env(args.iter().map(|arg| arg.to_string()), move |key| {
            env_map
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        })
    }

    fn opts(parsed: Parsed) -> Options {
        match parsed {
            Parsed::Run(options) => options,
            other => panic!("expected run, got {other:?}"),
        }
    }

    #[test]
    fn defaults() {
        let options = opts(parse(&[], &[]).unwrap());
        assert_eq!(options.bind.to_string(), "127.0.0.1");
        assert_eq!(options.port, 8080);
        assert_eq!(options.conf, PathBuf::from("/etc/samba/smb.conf"));
        assert!(options.user.is_none());
        assert_eq!(
            options.reload_cmd.as_deref(),
            Some("smbcontrol all reload-config")
        );
        assert_eq!(options.testparm, Some(PathBuf::from("testparm")));
        assert_eq!(options.max_backups, 10);
        assert_eq!(
            options.backup_dir,
            PathBuf::from("/etc/samba/samba-ui-backups")
        );
        assert!(!options.read_only);
        assert!(options.allow_origins.is_empty());
    }

    #[test]
    fn overrides_flags_and_inline_values() {
        let options = opts(
            parse(
                &[
                    "--bind=127.0.0.1",
                    "--port",
                    "9000",
                    "--conf",
                    "C:/tmp/smb.conf",
                    "--user",
                    "admin",
                    "--password",
                    "pw",
                    "--no-reload",
                    "--no-testparm",
                    "--backups",
                    "0",
                    "--read-only",
                ],
                &[],
            )
            .unwrap(),
        );
        assert_eq!(options.port, 9000);
        assert_eq!(options.conf, PathBuf::from("C:/tmp/smb.conf"));
        assert_eq!(options.user.as_deref(), Some("admin"));
        assert!(options.reload_cmd.is_none());
        assert!(options.testparm.is_none());
        assert_eq!(options.max_backups, 0);
        assert!(options.read_only);
    }

    #[test]
    fn allow_origins_from_flags_and_env() {
        let options = opts(
            parse(
                &[
                    "--allow-origin",
                    "http://nas:8080/",
                    "--allow-origin=https://nas",
                ],
                &[],
            )
            .unwrap(),
        );
        assert_eq!(
            options.allow_origins,
            vec!["http://nas:8080".to_string(), "https://nas".to_string()]
        );

        let options = opts(
            parse(
                &[],
                &[("SAMBA_UI_ALLOW_ORIGINS", "http://a:8080, http://b")],
            )
            .unwrap(),
        );
        assert_eq!(
            options.allow_origins,
            vec!["http://a:8080".to_string(), "http://b".to_string()]
        );

        let options = opts(
            parse(
                &["--allow-origin", "http://c,http://d"],
                &[("SAMBA_UI_ALLOW_ORIGINS", "http://a")],
            )
            .unwrap(),
        );
        assert_eq!(
            options.allow_origins,
            vec![
                "http://a".to_string(),
                "http://c".to_string(),
                "http://d".to_string()
            ]
        );
    }

    #[test]
    fn env_is_overridden_by_flags() {
        let options = opts(
            parse(
                &["--port", "9001"],
                &[("SAMBA_UI_PORT", "9000"), ("SAMBA_UI_CONF", "/x/smb.conf")],
            )
            .unwrap(),
        );
        assert_eq!(options.port, 9001);
        assert_eq!(options.conf, PathBuf::from("/x/smb.conf"));
    }

    #[test]
    fn empty_reload_disables_it() {
        let options = opts(parse(&["--reload="], &[]).unwrap());
        assert!(options.reload_cmd.is_none());
    }

    #[test]
    fn rejects_non_loopback_without_auth() {
        let err = parse(&["--bind", "0.0.0.0"], &[]).unwrap_err();
        assert!(err.contains("无认证"), "{err}");

        let options = opts(parse(&["--bind", "0.0.0.0", "--allow-insecure"], &[]).unwrap());
        assert_eq!(options.bind.to_string(), "0.0.0.0");
    }

    #[test]
    fn auth_validation() {
        assert!(parse(&["--user", "admin"], &[]).is_err());
        assert!(parse(&["--password", "pw"], &[]).is_err());
        assert!(parse(&["--user", "admin", "--password", ""], &[]).is_err());
        let options = opts(
            parse(
                &["--bind", "0.0.0.0", "--user", "admin", "--password", "pw"],
                &[],
            )
            .unwrap(),
        );
        assert_eq!(options.user.as_deref(), Some("admin"));
    }

    #[test]
    fn no_auth_clears_credentials() {
        let options =
            opts(parse(&["--user", "admin", "--password", "pw", "--no-auth"], &[]).unwrap());
        assert!(options.user.is_none());
        assert!(options.password.is_none());
    }

    #[test]
    fn rejects_bad_values() {
        assert!(parse(&["--port", "0"], &[]).is_err());
        assert!(parse(&["--port", "abc"], &[]).is_err());
        assert!(parse(&["--bind", "example.com"], &[]).is_err());
        assert!(parse(&["--unknown"], &[]).is_err());
        assert!(parse(&["--port"], &[]).is_err());
    }

    #[test]
    fn localhost_is_normalized() {
        let options = opts(parse(&["--bind", "localhost"], &[]).unwrap());
        assert!(options.bind.is_loopback());
    }

    #[test]
    fn help_and_version() {
        assert!(matches!(parse(&["--help"], &[]).unwrap(), Parsed::Help));
        assert!(matches!(parse(&["-V"], &[]).unwrap(), Parsed::Version));
        assert!(help_text().contains("--conf"));
    }
}
