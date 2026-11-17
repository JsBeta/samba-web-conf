//! 服务装配、监听循环与请求日志。

use std::fs;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use crate::args::{self, Options};
use crate::html::View;
use crate::http::{self, status_code, Response};
use crate::routes;
use crate::store::{
    Loaded, NoReloader, NoValidator, Reloader, ShellReloader, Store, Testparm, Validator,
};
use crate::util;
use crate::VERSION;

const MAX_CONNECTIONS: usize = 64;
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
const THREAD_STACK_SIZE: usize = 512 * 1024;

pub struct App {
    pub opts: Options,
    pub store: Store,
    pub auth: Option<(String, String)>,
    pub csrf: String,
    pub read_only: bool,
    pub warnings: Vec<String>,
    pub conf_display: String,
    pub validator_label: String,
    pub reload_label: String,
    pub backup_dir_display: String,
    pub pdbedit: Option<PathBuf>,
    pub smbpasswd: Option<PathBuf>,
    saves: Mutex<()>,
    conns: AtomicUsize,
}

impl App {
    pub fn new(opts: Options) -> Result<App, String> {
        let conf = fs::canonicalize(&opts.conf).map_err(|err| {
            format!(
                "无法打开配置文件 {}：{err}\n提示：用 --conf 指定 smb.conf 的实际路径",
                opts.conf.display()
            )
        })?;
        let metadata = fs::metadata(&conf)
            .map_err(|err| format!("无法读取文件属性 {}：{err}", conf.display()))?;
        if !metadata.is_file() {
            return Err(format!("{} 不是普通文件", conf.display()));
        }

        let mut warnings = Vec::new();
        let validator = detect_validator(&opts.testparm, &mut warnings);
        let reload = detect_reloader(&opts.reload_cmd, &mut warnings);
        let auth = match (&opts.user, &opts.password) {
            (Some(user), Some(password)) if !user.is_empty() && !password.is_empty() => {
                Some((user.clone(), password.clone()))
            }
            _ => None,
        };
        if auth.is_none() {
            warnings.push(
                "未启用登录认证：请只监听回环地址，或用 --user/--password 开启 Basic 认证。"
                    .to_string(),
            );
        }
        if opts.read_only {
            warnings.push("当前为只读模式：所有保存、创建、删除操作均被拒绝。".to_string());
        }
        if !opts.allow_origins.is_empty() {
            warnings.push(format!(
                "已允许额外 Origin：{}（仅用于反向代理改写 Host 的场景）",
                opts.allow_origins.join("、")
            ));
        }
        if opts.max_backups == 0 {
            warnings.push("已禁用自动备份（--backups 0），保存前不会生成备份。".to_string());
        }
        if metadata.permissions().readonly() {
            warnings.push(format!(
                "配置文件带有只读权限标记：{}（root 仍可能写入）",
                conf.display()
            ));
        }

        let pdbedit = util::find_in_path("pdbedit");
        let smbpasswd = util::find_in_path("smbpasswd");
        if pdbedit.is_none() || smbpasswd.is_none() {
            warnings.push("未找到 pdbedit 或 smbpasswd：用户管理不可用。".to_string());
        }

        let store = Store::new(
            conf.clone(),
            opts.backup_dir.clone(),
            opts.max_backups,
            validator,
            reload,
        );
        let validator_label = store.validator_label();
        let reload_label = store.reload_label();
        let conf_display = conf.display().to_string();
        let backup_dir_display = opts.backup_dir.display().to_string();
        let read_only = opts.read_only;

        Ok(App {
            opts,
            store,
            auth,
            csrf: util::csrf_token(),
            read_only,
            warnings,
            conf_display,
            validator_label,
            reload_label,
            backup_dir_display,
            pdbedit,
            smbpasswd,
            saves: Mutex::new(()),
            conns: AtomicUsize::new(0),
        })
    }

    pub fn view(&self) -> View<'_> {
        View::new(
            &self.conf_display,
            self.auth.is_some(),
            self.read_only,
            &self.validator_label,
            &self.reload_label,
            &self.backup_dir_display,
            self.store.backup_count(),
            &self.warnings,
        )
        .with_csrf(&self.csrf)
        .with_users(self.pdbedit.is_some() && self.smbpasswd.is_some())
    }

    pub fn lock_saves(&self) -> MutexGuard<'_, ()> {
        match self.saves.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub fn load(&self) -> Result<Loaded, String> {
        self.store.load()
    }
}

fn detect_validator(requested: &Option<PathBuf>, warnings: &mut Vec<String>) -> Box<dyn Validator> {
    let Some(path) = requested else {
        return Box::new(NoValidator);
    };
    let explicit = path.is_absolute() || path.components().count() > 1;
    let resolved = if explicit {
        if path.is_file() {
            Some(path.clone())
        } else {
            None
        }
    } else {
        util::find_in_path(&path.to_string_lossy())
    };
    match resolved {
        Some(bin) => Box::new(Testparm { bin }),
        None => {
            warnings.push(format!(
                "未找到 testparm（{}）：保存时跳过 Samba 语法校验。",
                path.display()
            ));
            Box::new(NoValidator)
        }
    }
}

fn detect_reloader(requested: &Option<String>, warnings: &mut Vec<String>) -> Box<dyn Reloader> {
    let Some(command) = requested else {
        return Box::new(NoReloader);
    };
    if command.trim().is_empty() {
        return Box::new(NoReloader);
    }
    if let Some(program) = command.split_whitespace().next() {
        if !program.contains('/')
            && !program.contains('\\')
            && util::find_in_path(program).is_none()
        {
            warnings.push(format!(
                "重载命令 {program} 不在 PATH 中，保存后可能无法自动重载。"
            ));
        }
    }
    Box::new(ShellReloader {
        command: command.clone(),
    })
}

struct ConnGuard<'a>(&'a AtomicUsize);

impl Drop for ConnGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn serve(listener: TcpListener, app: Arc<App>) -> io::Result<()> {
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let current = app.conns.fetch_add(1, Ordering::SeqCst);
                if current >= MAX_CONNECTIONS {
                    app.conns.fetch_sub(1, Ordering::SeqCst);
                    let mut stream = stream;
                    let _ = http::write_response(
                        &mut stream,
                        &Response::text(
                            status_code::SERVICE_UNAVAILABLE,
                            "服务器繁忙，请稍后再试\n",
                        ),
                        true,
                    );
                    continue;
                }
                let child_app = Arc::clone(&app);
                let spawned = thread::Builder::new()
                    .name("samba-ui-http".to_string())
                    .stack_size(THREAD_STACK_SIZE)
                    .spawn(move || {
                        let _guard = ConnGuard(&child_app.conns);
                        handle_connection(stream, &child_app);
                    });
                if spawned.is_err() {
                    app.conns.fetch_sub(1, Ordering::SeqCst);
                    util::warn("无法创建连接处理线程");
                }
            }
            Err(err) => util::warn(format!("接受连接失败: {err}")),
        }
    }
    Ok(())
}

fn handle_connection(mut stream: TcpStream, app: &Arc<App>) {
    let peer = stream
        .peer_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| "-".to_string());
    let _ = stream.set_read_timeout(Some(CONNECTION_TIMEOUT));
    let _ = stream.set_write_timeout(Some(CONNECTION_TIMEOUT));
    let started = Instant::now();

    match http::read_request(&mut stream) {
        Ok(request) => {
            let is_head = request.method == "HEAD";
            let response = routes::handle(app, &request);
            let status = response.status;
            if let Err(err) = http::write_response(&mut stream, &response, !is_head) {
                util::warn(format!("{peer} 写响应失败: {err}"));
            }
            util::info(format!(
                "{peer} {} {} -> {} ({} ms)",
                request.method,
                request.path,
                status,
                started.elapsed().as_millis()
            ));
        }
        Err(http::ReadError::Bad(message)) => {
            let response = Response::text(
                status_code::BAD_REQUEST,
                format!("400 Bad Request: {message}"),
            );
            let _ = http::write_response(&mut stream, &response, true);
            util::warn(format!("{peer} 非法请求: {message}"));
        }
        Err(http::ReadError::Io(err)) => {
            util::warn(format!("{peer} 读取请求失败: {err}"));
        }
    }
}

pub fn run<I: IntoIterator<Item = String>>(args: I) -> Result<(), String> {
    match args::parse_args(args)? {
        args::Parsed::Help => {
            print!("{}", args::help_text());
            Ok(())
        }
        args::Parsed::Version => {
            println!("samba-ui {VERSION} ({})", std::env::consts::ARCH);
            Ok(())
        }
        args::Parsed::Run(options) => {
            let app = Arc::new(App::new(options)?);
            let address = SocketAddr::new(app.opts.bind, app.opts.port);
            let listener =
                TcpListener::bind(address).map_err(|err| format!("无法监听 {address}：{err}"))?;
            let local = listener.local_addr().unwrap_or(address);

            util::info(format!("samba-ui v{VERSION} 已启动: http://{local}/"));
            util::info(format!("配置文件: {}", app.conf_display));
            util::info(format!(
                "语法校验: {}；保存后重载: {}",
                app.validator_label, app.reload_label
            ));
            for warning in &app.warnings {
                util::warn(warning);
            }
            if app.auth.is_none() && !app.opts.bind.is_loopback() {
                util::warn("正在非回环地址上无认证运行，请确认网络边界。");
            }

            serve(listener, app).map_err(|err| format!("服务异常退出: {err}"))
        }
    }
}
