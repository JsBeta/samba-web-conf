//! 极简 HTTP/1.1 服务端实现：请求解析、表单解码、Basic 认证与响应输出。

use std::fmt;
use std::io::{Read, Write};
use std::net::TcpStream;

use crate::util;

pub const MAX_HEADER_BYTES: usize = 16 * 1024;
pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;
const MAX_FORM_FIELDS: usize = 4096;

#[derive(Debug)]
pub enum ReadError {
    Bad(String),
    Io(std::io::Error),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::Bad(msg) => write!(f, "{msg}"),
            ReadError::Io(err) => write!(f, "网络读取失败: {err}"),
        }
    }
}

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Vec<u8>,
    headers: Vec<(String, String)>,
}

struct Head {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    content_length: usize,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// 路径分段（逐段做百分号解码），用于路由。
    pub fn segments(&self) -> Result<Vec<String>, String> {
        let trimmed = self.path.trim_start_matches('/');
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for part in trimmed.split('/') {
            if part.is_empty() {
                continue;
            }
            out.push(percent_decode(part, false)?);
        }
        Ok(out)
    }

    pub fn form(&self) -> Result<Vec<(String, String)>, String> {
        parse_pairs(std::str::from_utf8(&self.body).map_err(|_| "表单不是有效的 UTF-8")?)
    }
}

pub fn read_request(stream: &mut TcpStream) -> Result<Request, ReadError> {
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    let mut chunk = [0u8; 8 * 1024];

    let (header_end, terminator_len) = loop {
        if let Some(found) = find_header_end(&buf) {
            break found;
        }
        if buf.len() > MAX_HEADER_BYTES {
            return Err(ReadError::Bad("请求头过大".to_string()));
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Err(ReadError::Bad("连接在请求头完成前关闭".to_string())),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(err) => return Err(ReadError::Io(err)),
        }
    };

    let head_text = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let head = parse_head(&head_text).map_err(ReadError::Bad)?;
    let total = header_end
        .saturating_add(terminator_len)
        .saturating_add(head.content_length);

    while buf.len() < total {
        let want = (total - buf.len()).min(chunk.len());
        match stream.read(&mut chunk[..want]) {
            Ok(0) => return Err(ReadError::Bad("请求体不完整".to_string())),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(err) => return Err(ReadError::Io(err)),
        }
    }

    let body_start = header_end + terminator_len;
    let body = buf[body_start..total].to_vec();
    build_request(head, body).map_err(ReadError::Bad)
}

/// 解析一段完整请求（含请求头与请求体），主要用于测试。
pub fn parse_request(bytes: &[u8]) -> Result<Request, String> {
    let (header_end, terminator_len) =
        find_header_end(bytes).ok_or_else(|| "请求头不完整".to_string())?;
    let head_text = String::from_utf8_lossy(&bytes[..header_end]).into_owned();
    let head = parse_head(&head_text)?;
    let body_start = header_end + terminator_len;
    if bytes.len() < body_start + head.content_length {
        return Err("请求体不完整".to_string());
    }
    let body = bytes[body_start..body_start + head.content_length].to_vec();
    build_request(head, body)
}

fn build_request(head: Head, body: Vec<u8>) -> Result<Request, String> {
    let (raw_path, raw_query) = match head.target.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (head.target.clone(), String::new()),
    };
    let path = normalize_target(&raw_path)?;
    let query = parse_pairs(&raw_query)?;
    Ok(Request {
        method: head.method,
        path,
        query,
        body,
        headers: head.headers,
    })
}

fn normalize_target(target: &str) -> Result<String, String> {
    if target.starts_with('/') {
        return Ok(target.to_string());
    }
    // 代理风格绝对地址：http://host/path
    for scheme in ["http://", "https://"] {
        if let Some(rest) = target.strip_prefix(scheme) {
            return match rest.find('/') {
                Some(index) => Ok(rest[index..].to_string()),
                None => Ok("/".to_string()),
            };
        }
    }
    Err("非法的请求路径".to_string())
}

fn parse_head(text: &str) -> Result<Head, String> {
    let mut lines = text.lines();
    let request_line = lines.next().ok_or_else(|| "缺少请求行".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "缺少请求方法".to_string())?
        .to_ascii_uppercase();
    let target = parts
        .next()
        .ok_or_else(|| "缺少请求路径".to_string())?
        .to_string();
    let version = parts.next().ok_or_else(|| "缺少 HTTP 版本".to_string())?;
    if !version.starts_with("HTTP/") {
        return Err("不是 HTTP 请求".to_string());
    }

    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| "非法的请求头".to_string())?;
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() {
            return Err("请求头名称为空".to_string());
        }
        headers.push((name, value.trim().to_string()));
        if headers.len() > 128 {
            return Err("请求头过多".to_string());
        }
    }

    let lookup = |name: &str| -> Option<&str> {
        headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };

    if let Some(encoding) = lookup("transfer-encoding") {
        if !encoding.trim().is_empty() {
            return Err("不支持分块传输编码".to_string());
        }
    }

    let content_length = match lookup("content-length") {
        Some(raw) => raw
            .parse::<usize>()
            .map_err(|_| "非法的 Content-Length".to_string())?,
        None => 0,
    };
    if content_length > MAX_BODY_BYTES {
        return Err("请求体过大".to_string());
    }

    if method == "POST" && lookup("content-length").is_none() {
        return Err("POST 请求缺少 Content-Length".to_string());
    }

    let expects_body = method == "POST";
    let content_length = if expects_body { content_length } else { 0 };

    Ok(Head {
        method,
        target,
        headers,
        content_length,
    })
}

fn find_header_end(buf: &[u8]) -> Option<(usize, usize)> {
    let crlf = buf.windows(4).position(|w| w == b"\r\n\r\n");
    let lf = buf.windows(2).position(|w| w == b"\n\n");
    match (crlf, lf) {
        (Some(a), Some(b)) => {
            if a <= b {
                Some((a, 4))
            } else {
                Some((b, 2))
            }
        }
        (Some(a), None) => Some((a, 4)),
        (None, Some(b)) => Some((b, 2)),
        (None, None) => None,
    }
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn percent_decode(input: &str, plus_as_space: bool) -> Result<String, String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err("非法的百分号编码".to_string());
                }
                let hi = from_hex(bytes[i + 1]).ok_or_else(|| "非法的百分号编码".to_string())?;
                let lo = from_hex(bytes[i + 2]).ok_or_else(|| "非法的百分号编码".to_string())?;
                out.push(hi * 16 + lo);
                i += 3;
            }
            b'+' if plus_as_space => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    if out.contains(&0) {
        return Err("包含 NUL 字节".to_string());
    }
    String::from_utf8(out).map_err(|_| "不是有效的 UTF-8".to_string())
}

pub fn url_encode(input: &str) -> String {
    const HEX: [char; 16] = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F',
    ];
    let mut out = String::with_capacity(input.len());
    for &byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => {
                out.push('%');
                out.push(HEX[(byte >> 4) as usize]);
                out.push(HEX[(byte & 0x0f) as usize]);
            }
        }
    }
    out
}

pub fn parse_pairs(input: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    if input.is_empty() {
        return Ok(out);
    }
    for pair in input.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = percent_decode(key, true)?;
        let value = percent_decode(value, true)?;
        if key.is_empty() {
            continue;
        }
        out.push((key, value));
        if out.len() > MAX_FORM_FIELDS {
            return Err("字段过多".to_string());
        }
    }
    Ok(out)
}

pub fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer: u32 = 0;
    let mut bits = 0u32;
    let mut padding = 0usize;

    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding += 1;
                continue;
            }
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        if padding > 0 {
            return None;
        }
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    if padding > 2 {
        return None;
    }
    Some(out)
}

pub fn basic_credentials(req: &Request) -> Option<(String, String)> {
    let header = req.header("authorization")?;
    let (scheme, token) = header.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let raw = base64_decode(token.trim())?;
    let text = String::from_utf8(raw).ok()?;
    let (user, password) = text.split_once(':')?;
    Some((user.to_string(), password.to_string()))
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    fn base(status: u16, content_type: &str, body: Vec<u8>) -> Response {
        let mut response = Response {
            status,
            headers: Vec::new(),
            body,
        };
        response
            .headers
            .push(("Content-Type".to_string(), content_type.to_string()));
        response
    }

    fn with_security_headers(mut self) -> Response {
        self.headers
            .push(("X-Content-Type-Options".to_string(), "nosniff".to_string()));
        self.headers
            .push(("Referrer-Policy".to_string(), "no-referrer".to_string()));
        self.headers
            .push(("X-Frame-Options".to_string(), "DENY".to_string()));
        self
    }

    pub fn html(status: u16, body: String) -> Response {
        Response::base(status, "text/html; charset=utf-8", body.into_bytes())
            .with_security_headers()
            .with_header(
                "Content-Security-Policy",
                "default-src 'none'; style-src 'self'; script-src 'self'; img-src 'self' data:; \
                 connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
            )
            .with_header("Cache-Control", "no-store")
    }

    pub fn text(status: u16, body: impl Into<String>) -> Response {
        Response::base(
            status,
            "text/plain; charset=utf-8",
            body.into().into_bytes(),
        )
        .with_security_headers()
        .with_header("Cache-Control", "no-store")
    }

    pub fn asset(content_type: &str, body: &'static str) -> Response {
        Response::base(status_code::OK, content_type, body.as_bytes().to_vec())
            .with_security_headers()
            .with_header("Cache-Control", "public, max-age=3600")
    }

    pub fn redirect(location: &str) -> Response {
        Response::base(
            status_code::SEE_OTHER,
            "text/plain; charset=utf-8",
            Vec::new(),
        )
        .with_header("Location", location)
        .with_header("Cache-Control", "no-store")
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Response {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

pub mod status_code {
    pub const OK: u16 = 200;
    pub const NO_CONTENT: u16 = 204;
    pub const SEE_OTHER: u16 = 303;
    pub const BAD_REQUEST: u16 = 400;
    pub const UNAUTHORIZED: u16 = 401;
    pub const FORBIDDEN: u16 = 403;
    pub const NOT_FOUND: u16 = 404;
    pub const METHOD_NOT_ALLOWED: u16 = 405;
    pub const CONFLICT: u16 = 409;
    pub const PAYLOAD_TOO_LARGE: u16 = 413;
    pub const INTERNAL_SERVER_ERROR: u16 = 500;
    pub const SERVICE_UNAVAILABLE: u16 = 503;
}

pub fn reason_phrase(status: u16) -> &'static str {
    match status {
        status_code::OK => "OK",
        status_code::NO_CONTENT => "No Content",
        status_code::SEE_OTHER => "See Other",
        status_code::BAD_REQUEST => "Bad Request",
        status_code::UNAUTHORIZED => "Unauthorized",
        status_code::FORBIDDEN => "Forbidden",
        status_code::NOT_FOUND => "Not Found",
        status_code::METHOD_NOT_ALLOWED => "Method Not Allowed",
        status_code::CONFLICT => "Conflict",
        status_code::PAYLOAD_TOO_LARGE => "Payload Too Large",
        status_code::INTERNAL_SERVER_ERROR => "Internal Server Error",
        status_code::SERVICE_UNAVAILABLE => "Service Unavailable",
        _ => "Unknown",
    }
}

pub fn response_bytes(response: &Response, include_body: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(512 + if include_body { response.body.len() } else { 0 });
    out.extend_from_slice(
        format!(
            "HTTP/1.1 {} {}\r\n",
            response.status,
            reason_phrase(response.status)
        )
        .as_bytes(),
    );
    out.extend_from_slice(format!("Date: {}\r\n", util::http_date(util::now_unix())).as_bytes());
    out.extend_from_slice(b"Server: samba-ui\r\n");
    out.extend_from_slice(b"Connection: close\r\n");
    out.extend_from_slice(format!("Content-Length: {}\r\n", response.body.len()).as_bytes());
    if response.header("Content-Type").is_none() {
        out.extend_from_slice(b"Content-Type: text/plain; charset=utf-8\r\n");
    }
    for (name, value) in &response.headers {
        out.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    if include_body {
        out.extend_from_slice(&response.body);
    }
    out
}

pub fn write_response(
    stream: &mut TcpStream,
    response: &Response,
    include_body: bool,
) -> std::io::Result<()> {
    let bytes = response_bytes(response, include_body);
    stream.write_all(&bytes)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_get_with_query() {
        let raw = b"GET /shares/my%20share?msg=saved&x=1 HTTP/1.1\r\nHost: localhost\r\nCookie: a=b\r\n\r\n";
        let req = parse_request(raw).unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/shares/my%20share");
        assert_eq!(
            req.query,
            vec![
                ("msg".to_string(), "saved".to_string()),
                ("x".to_string(), "1".to_string())
            ]
        );
        assert_eq!(req.segments().unwrap(), vec!["shares", "my share"]);
        assert_eq!(req.header("HOST"), Some("localhost"));
        assert!(req.body.is_empty());
    }

    #[test]
    fn parses_post_form() {
        let body = "c.0=WORKGROUP&k.1=path&v.1=%2Fsrv%2Fshare&_csrf=abc";
        let raw = format!(
            "POST /global HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let req = parse_request(raw.as_bytes()).unwrap();
        assert_eq!(req.method, "POST");
        assert_eq!(
            req.form().unwrap(),
            vec![
                ("c.0".to_string(), "WORKGROUP".to_string()),
                ("k.1".to_string(), "path".to_string()),
                ("v.1".to_string(), "/srv/share".to_string()),
                ("_csrf".to_string(), "abc".to_string())
            ]
        );
    }

    #[test]
    fn rejects_bad_requests() {
        assert!(parse_request(b"POST / HTTP/1.1\r\nHost: x\r\n\r\n").is_err());
        assert!(
            parse_request(b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n")
                .is_err()
        );
        assert!(parse_request(b"GET relative HTTP/1.1\r\n\r\n").is_err());
        assert!(parse_request(b"not http\r\n\r\n").is_err());
        assert!(
            parse_request(b"GET / HTTP/1.1\r\nHost: x\r\n\r\nGET / HTTP/1.1\r\nHost: x\r\n\r\n")
                .unwrap()
                .path
                == "/"
        );
    }

    #[test]
    fn percent_and_url_encoding() {
        assert_eq!(percent_decode("a+b", true).unwrap(), "a b");
        assert_eq!(percent_decode("a+b", false).unwrap(), "a+b");
        assert_eq!(percent_decode("%E4%B8%AD%E6%96%87", false).unwrap(), "中文");
        assert!(percent_decode("%zz", false).is_err());
        assert!(percent_decode("%2", false).is_err());
        assert!(percent_decode("a%00b", false).is_err());

        let text = "共享 a/b?c&d=中";
        let encoded = url_encode(text);
        assert_eq!(percent_decode(&encoded, false).unwrap(), text);
        assert!(encoded.contains("%2F"));
    }

    #[test]
    fn base64_vectors() {
        assert_eq!(base64_decode("Zm9v").unwrap(), b"foo");
        assert_eq!(base64_decode("Zg==").unwrap(), b"f");
        assert_eq!(base64_decode("Zm8=").unwrap(), b"fo");
        assert_eq!(base64_decode("dXNlcjpwYXNz").unwrap(), b"user:pass");
        assert!(base64_decode("!!!").is_none());
        assert!(base64_decode("Zg==Zg==").is_none());
    }

    #[test]
    fn basic_auth_parsing() {
        let raw = b"GET / HTTP/1.1\r\nAuthorization: Basic dXNlcjpwYXNz\r\n\r\n";
        let req = parse_request(raw).unwrap();
        let (user, password) = basic_credentials(&req).unwrap();
        assert_eq!(user, "user");
        assert_eq!(password, "pass");
    }

    #[test]
    fn response_has_security_headers() {
        let response = Response::html(status_code::OK, "<p>hi</p>".to_string());
        let bytes = response_bytes(&response, true);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 9\r\n"));
        assert!(text.contains("Content-Security-Policy:"));
        assert!(text.contains("X-Content-Type-Options: nosniff"));
        assert!(text.ends_with("\r\n\r\n<p>hi</p>"));
    }

    #[test]
    fn head_response_has_no_body() {
        let response = Response::text(status_code::OK, "hello");
        let bytes = response_bytes(&response, false);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.ends_with("\r\n\r\n"));
        assert!(text.contains("Content-Length: 5"));
    }
}
