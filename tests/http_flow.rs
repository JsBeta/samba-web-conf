//! 端到端 HTTP 流程测试：在随机端口启动真实服务并走完整表单流程。

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use samba_ui::app::{serve, App};
use samba_ui::args::Options;
use samba_ui::http::url_encode;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> TempDir {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let name = format!("samba-ui-{prefix}-{}-{stamp}", std::process::id());
        let candidates = [
            std::env::var("SAMBA_UI_TEST_TMP").ok().map(PathBuf::from),
            Some(std::env::temp_dir()),
            Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/test-tmp")),
        ];
        let mut last_error = None;
        for base in candidates.into_iter().flatten() {
            let path = base.join(&name);
            match std::fs::create_dir_all(&path) {
                Ok(()) => return TempDir { path },
                Err(err) => last_error = Some(err),
            }
        }
        panic!("无法创建测试临时目录: {last_error:?}");
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

struct TestResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl TestResponse {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn exchange(address: SocketAddr, raw: &str) -> TestResponse {
    let mut stream = TcpStream::connect(address).expect("连接测试服务器失败");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(raw.as_bytes()).unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let text = String::from_utf8_lossy(&bytes).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_string(), value.trim().to_string()))
        .collect();
    TestResponse {
        status,
        headers,
        body: body.to_string(),
    }
}

fn auth_header() -> &'static str {
    // admin:secret
    "Authorization: Basic YWRtaW46c2VjcmV0\r\n"
}

fn get(address: SocketAddr, path: &str, auth: bool) -> TestResponse {
    let auth = if auth { auth_header() } else { "" };
    exchange(
        address,
        &format!("GET {path} HTTP/1.1\r\nHost: {address}\r\n{auth}Connection: close\r\n\r\n"),
    )
}

fn post(
    address: SocketAddr,
    path: &str,
    body: &str,
    auth: bool,
    origin: Option<&str>,
) -> TestResponse {
    let auth = if auth { auth_header() } else { "" };
    let origin = origin
        .map(|value| format!("Origin: {value}\r\n"))
        .unwrap_or_default();
    exchange(
        address,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: {address}\r\n{auth}{origin}\
             Content-Type: application/x-www-form-urlencoded\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn post_custom(
    address: SocketAddr,
    path: &str,
    body: &str,
    host: &str,
    origin: Option<&str>,
    forwarded_host: Option<&str>,
    auth: bool,
) -> TestResponse {
    let auth = if auth { auth_header() } else { "" };
    let origin = origin
        .map(|value| format!("Origin: {value}\r\n"))
        .unwrap_or_default();
    let forwarded = forwarded_host
        .map(|value| format!("X-Forwarded-Host: {value}\r\n"))
        .unwrap_or_default();
    exchange(
        address,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\n{auth}{origin}{forwarded}\
             Content-Type: application/x-www-form-urlencoded\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{}={}", url_encode(key), url_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn hidden_field(html: &str, name: &str) -> String {
    let needle = format!("name=\"{name}\" value=\"");
    let start = html
        .find(&needle)
        .unwrap_or_else(|| panic!("页面中没有 {name}"))
        + needle.len();
    let rest = &html[start..];
    let end = rest.find('"').expect("隐藏字段未闭合");
    rest[..end].to_string()
}

fn csrf_and_rev(html: &str) -> (String, String) {
    (hidden_field(html, "_csrf"), hidden_field(html, "_rev"))
}

fn start(app: App) -> (SocketAddr, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = std::thread::spawn(move || {
        let _ = serve(listener, Arc::new(app));
    });
    (address, handle)
}

fn options(conf: &Path, backup_dir: &Path) -> Options {
    Options {
        bind: "127.0.0.1".parse().unwrap(),
        port: 0,
        conf: conf.to_path_buf(),
        user: Some("admin".to_string()),
        password: Some("secret".to_string()),
        reload_cmd: None,
        testparm: None,
        backup_dir: backup_dir.to_path_buf(),
        max_backups: 3,
        allow_insecure: false,
        read_only: false,
        allow_origins: Vec::new(),
    }
}

#[test]
fn full_http_flow() {
    let dir = TempDir::new("flow");
    let conf = dir.path().join("smb.conf");
    std::fs::write(
        &conf,
        "# 保留这条注释\n[global]\n   workgroup = WORKGROUP\n\n\
         [public]\n   comment = 公共\n   path = /srv/public\n",
    )
    .unwrap();
    let backups = dir.path().join("backups");
    let app = App::new(options(&conf, &backups)).unwrap();
    let (address, _handle) = start(app);

    let response = get(address, "/", false);
    assert_eq!(response.status, 401);
    assert!(response.header("WWW-Authenticate").is_some());

    let response = get(address, "/", true);
    assert_eq!(response.status, 200);
    assert!(response.body.contains("概览"));
    assert!(response.body.contains("公共"));

    let response = get(address, "/global", true);
    assert_eq!(response.status, 200);
    let (csrf, rev) = csrf_and_rev(&response.body);
    assert_eq!(csrf.len(), 32);

    let response = post(address, "/global", "c.0=X", true, None);
    assert_eq!(response.status, 403);
    assert!(response.body.contains("CSRF"));

    let body = form(&[("_csrf", &csrf), ("_rev", &rev), ("c.0", "X")]);
    let response = post(address, "/global", &body, true, Some("http://evil.example"));
    assert_eq!(response.status, 403);

    let body = form(&[("_csrf", &csrf), ("_rev", &rev), ("c.0", "HOMELAB")]);
    let response = post(address, "/global", &body, true, None);
    assert_eq!(response.status, 303);
    assert_eq!(
        response.header("Location"),
        Some("/global?msg=saved_global")
    );
    let saved = std::fs::read_to_string(&conf).unwrap();
    assert!(saved.contains("# 保留这条注释"));
    assert!(saved.contains("workgroup = HOMELAB"));
    assert!(std::fs::read_dir(&backups).unwrap().count() >= 1);

    let response = get(address, "/global", true);
    assert_eq!(response.status, 200);
    let (csrf, new_rev) = csrf_and_rev(&response.body);
    assert_ne!(rev, new_rev);
    let body = form(&[("_csrf", &csrf), ("_rev", &rev), ("c.0", "STALE")]);
    let response = post(address, "/global", &body, true, None);
    assert_eq!(response.status, 409);

    let response = get(address, "/shares/new", true);
    let (csrf, rev) = csrf_and_rev(&response.body);
    let body = form(&[
        ("_csrf", &csrf),
        ("_rev", &rev),
        ("name", "docs"),
        ("c.0", "文档"),
        ("c.1", "/srv/docs"),
    ]);
    let response = post(address, "/shares/new", &body, true, None);
    assert_eq!(response.status, 303);
    assert_eq!(
        response.header("Location"),
        Some("/shares/docs?msg=created_share")
    );
    let saved = std::fs::read_to_string(&conf).unwrap();
    assert!(saved.contains("[docs]"));
    assert!(saved.contains("path = /srv/docs"));

    let response = get(address, "/shares/docs", true);
    assert_eq!(response.status, 200);
    assert!(response.body.contains("/srv/docs"));
    let (csrf, rev) = csrf_and_rev(&response.body);
    let body = form(&[("_csrf", &csrf), ("_rev", &rev), ("c.0", "新说明")]);
    let response = post(address, "/shares/docs", &body, true, None);
    assert_eq!(response.status, 303);
    let saved = std::fs::read_to_string(&conf).unwrap();
    assert!(saved.contains("comment = 新说明"));
    assert!(saved.contains("path = /srv/docs"));

    let response = get(address, "/shares/docs", true);
    let (csrf, rev) = csrf_and_rev(&response.body);
    let body = form(&[("_csrf", &csrf), ("_rev", &rev)]);
    let response = post(address, "/shares/docs/delete", &body, true, None);
    assert_eq!(response.status, 303);
    let saved = std::fs::read_to_string(&conf).unwrap();
    assert!(!saved.contains("[docs]"));

    let response = get(address, "/raw", true);
    let (csrf, rev) = csrf_and_rev(&response.body);
    let content = format!("{saved}# 原始编辑\n");
    let body = form(&[("_csrf", &csrf), ("_rev", &rev), ("content", &content)]);
    let response = post(address, "/raw", &body, true, None);
    assert_eq!(response.status, 303);
    let saved = std::fs::read_to_string(&conf).unwrap();
    assert!(saved.ends_with("# 原始编辑\n"));

    let response = exchange(
        address,
        &format!(
            "DELETE / HTTP/1.1\r\nHost: {address}\r\n{auth}Connection: close\r\n\r\n",
            auth = auth_header()
        ),
    );
    assert_eq!(response.status, 405);
}

#[test]
fn read_only_mode_rejects_writes() {
    let dir = TempDir::new("readonly");
    let conf = dir.path().join("smb.conf");
    std::fs::write(&conf, "[global]\n   workgroup = WORKGROUP\n").unwrap();
    let mut options = options(&conf, &dir.path().join("backups"));
    options.user = None;
    options.password = None;
    options.read_only = true;
    let app = App::new(options).unwrap();
    let (address, _handle) = start(app);

    let response = get(address, "/", false);
    assert_eq!(response.status, 200);
    assert!(response.body.contains("只读模式"));

    let response = post(address, "/global", "c.0=X", false, None);
    assert_eq!(response.status, 403);
    assert_eq!(
        std::fs::read_to_string(&conf).unwrap(),
        "[global]\n   workgroup = WORKGROUP\n"
    );
}

#[test]
fn forwarded_host_is_accepted() {
    let dir = TempDir::new("forwarded");
    let conf = dir.path().join("smb.conf");
    std::fs::write(&conf, "[global]\n   workgroup = WORKGROUP\n").unwrap();
    let mut options = options(&conf, &dir.path().join("backups"));
    options.user = None;
    options.password = None;
    let app = App::new(options).unwrap();
    let (address, _handle) = start(app);

    let response = get(address, "/global", false);
    assert_eq!(response.status, 200);
    let (csrf, rev) = csrf_and_rev(&response.body);
    let body = form(&[("_csrf", &csrf), ("_rev", &rev), ("c.0", "PROXY")]);

    let response = post_custom(
        address,
        "/global",
        &body,
        &address.to_string(),
        Some("http://nas:8080"),
        None,
        false,
    );
    assert_eq!(response.status, 403);
    assert!(response.body.contains("X-Forwarded-Host"));
    assert!(response.body.contains("--allow-origin"));

    let response = post_custom(
        address,
        "/global",
        &body,
        &address.to_string(),
        Some("http://nas:8080"),
        Some("nas:8080"),
        false,
    );
    assert_eq!(response.status, 303);
    assert!(std::fs::read_to_string(&conf)
        .unwrap()
        .contains("workgroup = PROXY"));
}

#[test]
fn allow_origin_option_accepts_mismatched_host() {
    let dir = TempDir::new("allow-origin");
    let conf = dir.path().join("smb.conf");
    std::fs::write(&conf, "[global]\n   workgroup = WORKGROUP\n").unwrap();
    let mut options = options(&conf, &dir.path().join("backups"));
    options.user = None;
    options.password = None;
    options.allow_origins = vec!["http://nas:8080".to_string()];
    let app = App::new(options).unwrap();
    let (address, _handle) = start(app);

    let response = get(address, "/global", false);
    let (csrf, rev) = csrf_and_rev(&response.body);
    let body = form(&[("_csrf", &csrf), ("_rev", &rev), ("c.0", "ALLOWED")]);
    let response = post_custom(
        address,
        "/global",
        &body,
        &address.to_string(),
        Some("http://nas:8080"),
        None,
        false,
    );
    assert_eq!(response.status, 303);
    assert!(std::fs::read_to_string(&conf)
        .unwrap()
        .contains("workgroup = ALLOWED"));

    // 默认端口应当等价：Host nas + Origin http://nas:80。
    let response = get(address, "/global", false);
    let (csrf, rev) = csrf_and_rev(&response.body);
    let body = form(&[("_csrf", &csrf), ("_rev", &rev), ("c.0", "PORT80")]);
    let response = post_custom(
        address,
        "/global",
        &body,
        "nas",
        Some("http://nas:80"),
        None,
        false,
    );
    assert_eq!(response.status, 303);
    assert!(std::fs::read_to_string(&conf)
        .unwrap()
        .contains("workgroup = PORT80"));
}

#[test]
fn backup_restore_flow() {
    let dir = TempDir::new("backup");
    let conf = dir.path().join("smb.conf");
    std::fs::write(&conf, "[global]\n   workgroup = WORKGROUP\n").unwrap();
    let mut options = options(&conf, &dir.path().join("backups"));
    options.user = None;
    options.password = None;
    let app = App::new(options).unwrap();
    let (address, _handle) = start(app);

    let response = get(address, "/global", false);
    let (csrf, rev) = csrf_and_rev(&response.body);
    let body = form(&[("_csrf", &csrf), ("_rev", &rev), ("c.0", "FIRST")]);
    let response = post(address, "/global", &body, false, None);
    assert_eq!(response.status, 303);

    let response = get(address, "/global", false);
    let (csrf, rev) = csrf_and_rev(&response.body);
    let body = form(&[("_csrf", &csrf), ("_rev", &rev), ("c.0", "SECOND")]);
    let response = post(address, "/global", &body, false, None);
    assert_eq!(response.status, 303);
    assert!(std::fs::read_to_string(&conf)
        .unwrap()
        .contains("workgroup = SECOND"));

    let response = get(address, "/backups", false);
    assert_eq!(response.status, 200);
    let backup = hidden_field(&response.body, "name");
    let (csrf, rev) = csrf_and_rev(&response.body);
    let body = form(&[("_csrf", &csrf), ("_rev", &rev), ("name", &backup)]);
    let response = post(address, "/backups/restore", &body, false, None);
    assert_eq!(response.status, 303);
    assert_eq!(
        response.header("Location"),
        Some("/backups?msg=backup_restored")
    );
    assert!(std::fs::read_to_string(&conf)
        .unwrap()
        .contains("workgroup = FIRST"));
}

#[test]
fn manual_reload_without_command_reports_error() {
    let dir = TempDir::new("reload");
    let conf = dir.path().join("smb.conf");
    std::fs::write(&conf, "[global]\n   workgroup = WORKGROUP\n").unwrap();
    let mut options = options(&conf, &dir.path().join("backups"));
    options.user = None;
    options.password = None;
    options.reload_cmd = None;
    let app = App::new(options).unwrap();
    let (address, _handle) = start(app);

    let response = get(address, "/global", false);
    let (csrf, _rev) = csrf_and_rev(&response.body);

    let response = post(address, "/reload", "no-csrf", false, None);
    assert_eq!(response.status, 403);
    assert!(response.body.contains("CSRF"));

    let body = form(&[("_csrf", &csrf)]);
    let response = post(address, "/reload", &body, false, None);
    assert_eq!(response.status, 400);
    assert!(response.body.contains("未配置重载命令"));
}

#[test]
fn null_origin_falls_back_to_csrf() {
    let dir = TempDir::new("null-origin");
    let conf = dir.path().join("smb.conf");
    std::fs::write(&conf, "[global]\n   workgroup = WORKGROUP\n").unwrap();
    let mut options = options(&conf, &dir.path().join("backups"));
    options.user = None;
    options.password = None;
    let app = App::new(options).unwrap();
    let (address, _handle) = start(app);

    let response = get(address, "/global", false);
    let (csrf, rev) = csrf_and_rev(&response.body);

    // Origin: null 不再直接拒绝，但仍必须带正确的 CSRF token。
    let response = post(address, "/global", "c.0=X", false, Some("null"));
    assert_eq!(response.status, 403);
    assert!(response.body.contains("CSRF"));

    let body = form(&[("_csrf", &csrf), ("_rev", &rev), ("c.0", "NULLOK")]);
    let response = post(address, "/global", &body, false, Some("null"));
    assert_eq!(response.status, 303);
    assert!(std::fs::read_to_string(&conf)
        .unwrap()
        .contains("workgroup = NULLOK"));
}
