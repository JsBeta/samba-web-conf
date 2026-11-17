//! 路由与表单处理。

use std::fs;
use std::time::Duration;

use crate::app::App;
use crate::conf::{self, Doc, Edit};
use crate::http::{self, status_code, url_encode, Request, Response};
use crate::pages::{self, Field, GLOBAL_FIELDS, NEW_SHARE_FIELDS, SHARE_FIELDS};
use crate::store::{CommitError, CommitInfo, Loaded};
use crate::users;
use crate::util::{self, RunOutcome};

const APP_CSS: &str = include_str!("../assets/app.css");
const APP_JS: &str = include_str!("../assets/app.js");
const AUTH_FAILURE_DELAY: Duration = Duration::from_millis(300);

pub fn handle(app: &App, request: &Request) -> Response {
    if let Some((user, password)) = &app.auth {
        let mut matches = false;
        if let Some((given_user, given_password)) = http::basic_credentials(request) {
            matches = util::ct_eq(given_user.as_bytes(), user.as_bytes())
                & util::ct_eq(given_password.as_bytes(), password.as_bytes());
        }
        if !matches {
            std::thread::sleep(AUTH_FAILURE_DELAY);
            return pages::unauthorized(&app.view());
        }
    }

    let segments = match request.segments() {
        Ok(segments) => segments,
        Err(err) => return pages::bad_request(&app.view(), &err),
    };
    let parts: Vec<&str> = segments.iter().map(String::as_str).collect();

    match request.method.as_str() {
        "GET" | "HEAD" => get(app, request, &parts),
        "POST" => {
            if app.read_only {
                return pages::forbidden(&app.view(), "当前为只读模式，已禁止保存。");
            }
            if let Err(detail) = origin_check(app, request) {
                util::warn(format!("拒绝跨站 POST: {}", detail.replace('\n', " | ")));
                return pages::error_page(
                    &app.view(),
                    status_code::FORBIDDEN,
                    "请求被拒绝",
                    "跨站请求被拒绝：Origin/Referer 与 Host 不匹配。",
                    Some(&detail),
                );
            }
            let form = match request.form() {
                Ok(form) => form,
                Err(err) => {
                    return pages::bad_request(&app.view(), &format!("表单解析失败：{err}"))
                }
            };
            if !csrf_ok(app, &form) {
                return pages::forbidden(&app.view(), "CSRF 校验失败，请返回并刷新页面后重试。");
            }
            post(app, &parts, &form)
        }
        _ => pages::method_not_allowed(&app.view()),
    }
}

fn get(app: &App, request: &Request, parts: &[&str]) -> Response {
    let view = app.view();
    match parts {
        [] => match load(app) {
            Ok(loaded) => Response::html(
                status_code::OK,
                pages::dashboard(&view, &loaded.doc, loaded.size, flash(request)),
            ),
            Err(response) => response,
        },
        ["healthz"] => Response::text(status_code::OK, "ok\n"),
        ["favicon.ico"] => Response::text(status_code::NO_CONTENT, ""),
        ["static", "app.css"] => Response::asset("text/css; charset=utf-8", APP_CSS),
        ["static", "app.js"] => Response::asset("application/javascript; charset=utf-8", APP_JS),
        ["global"] => match load(app) {
            Ok(loaded) => Response::html(
                status_code::OK,
                pages::global_page(
                    &view,
                    &app.csrf,
                    &loaded.doc,
                    &loaded.rev,
                    flash(request),
                    None,
                    None,
                ),
            ),
            Err(response) => response,
        },
        ["shares", "new"] => match load(app) {
            Ok(loaded) => Response::html(
                status_code::OK,
                pages::new_share_page(&view, &app.csrf, &loaded.doc, &loaded.rev, None, None, None),
            ),
            Err(response) => response,
        },
        ["shares", name] => match load(app) {
            Ok(loaded) => match pages::share_page(
                &view,
                &app.csrf,
                &loaded.doc,
                name,
                &loaded.rev,
                flash(request),
                None,
                None,
            ) {
                Some(page) => Response::html(status_code::OK, page),
                None => pages::not_found(&view),
            },
            Err(response) => response,
        },
        ["users"] => users_response(app, flash(request), None, None),
        ["users", name] => user_response(app, name, flash(request), None, None),
        ["raw"] => match load(app) {
            Ok(loaded) => Response::html(
                status_code::OK,
                pages::raw_page(
                    &view,
                    &app.csrf,
                    &loaded.text,
                    &loaded.rev,
                    flash(request),
                    None,
                    None,
                ),
            ),
            Err(response) => response,
        },
        _ => pages::not_found(&view),
    }
}

fn post(app: &App, parts: &[&str], form: &[(String, String)]) -> Response {
    match parts {
        ["global"] => save_global(app, form),
        ["shares", "new"] => create_share(app, form),
        ["shares", name] => save_share(app, name, form),
        ["shares", name, "delete"] => delete_share(app, name, form),
        ["users", "new"] => create_samba_user(app, form),
        ["users", name, "password"] => change_samba_password(app, name, form),
        ["users", name, "delete"] => delete_samba_user(app, name),
        ["users", name, "toggle"] => toggle_samba_user(app, name, form),
        ["raw"] => save_raw(app, form),
        ["reload"] => reload_samba(app),
        _ => pages::not_found(&app.view()),
    }
}

fn reload_samba(app: &App) -> Response {
    let _guard = app.lock_saves();

    if let Err(detail) = app.store.validator.validate(&app.store.conf) {
        util::warn(format!("手动重载前配置校验失败: {detail}"));
        return pages::error_page(
            &app.view(),
            status_code::BAD_REQUEST,
            "配置校验未通过",
            "未执行重载，Samba 仍在使用旧配置。请修正配置后重试。",
            Some(&detail),
        );
    }

    match app.store.reload.reload() {
        None => pages::error_page(
            &app.view(),
            status_code::BAD_REQUEST,
            "未配置重载命令",
            "当前以 --no-reload 运行，或 --reload 为空。请先配置重载命令。",
            None,
        ),
        Some(outcome) if outcome.success() => {
            util::info("已手动重载 Samba 配置");
            Response::redirect("/?msg=reloaded")
        }
        Some(outcome) => {
            util::warn(format!("手动重载失败: {}", outcome.summary()));
            Response::html(
                status_code::INTERNAL_SERVER_ERROR,
                pages::reload_manual_failure_page(&app.view(), &outcome),
            )
        }
    }
}

fn samba_users(app: &App) -> Result<Vec<users::SambaUser>, String> {
    let pdbedit = app
        .pdbedit
        .as_ref()
        .ok_or_else(|| "未找到 pdbedit，请先安装 Samba 工具。".to_string())?;
    let list = util::run_program("pdbedit -L", pdbedit, &["-L"], Duration::from_secs(15))?;
    if !list.success() {
        return Err(format!(
            "pdbedit -L 失败（{}）\n{}",
            list.summary(),
            list.output
        ));
    }
    let verbose = util::run_program("pdbedit -Lv", pdbedit, &["-Lv"], Duration::from_secs(15))
        .ok()
        .filter(|outcome| outcome.success())
        .map(|outcome| outcome.output)
        .unwrap_or_default();
    Ok(users::merge(&list.output, &verbose))
}

fn linux_candidates(current: &[users::SambaUser]) -> Vec<users::Candidate> {
    let passwd = fs::read_to_string("/etc/passwd").unwrap_or_default();
    users::linux_candidates(&passwd, current)
}

fn users_response(
    app: &App,
    flash_code: Option<&str>,
    error: Option<&str>,
    detail: Option<&str>,
) -> Response {
    match samba_users(app) {
        Ok(current) => {
            let candidates = linux_candidates(&current);
            Response::html(
                status_code::OK,
                pages::users_page(
                    &app.view(),
                    &app.csrf,
                    &current,
                    &candidates,
                    error,
                    detail,
                    flash_code,
                ),
            )
        }
        Err(message) => pages::error_page(
            &app.view(),
            status_code::INTERNAL_SERVER_ERROR,
            "读取 Samba 用户失败",
            &message,
            None,
        ),
    }
}

fn user_response(
    app: &App,
    name: &str,
    flash_code: Option<&str>,
    error: Option<&str>,
    detail: Option<&str>,
) -> Response {
    match samba_users(app) {
        Ok(current) => match current.iter().find(|user| user.name == name) {
            Some(user) => Response::html(
                status_code::OK,
                pages::user_page(&app.view(), &app.csrf, user, flash_code, error, detail),
            ),
            None => pages::not_found(&app.view()),
        },
        Err(message) => pages::error_page(
            &app.view(),
            status_code::INTERNAL_SERVER_ERROR,
            "读取 Samba 用户失败",
            &message,
            None,
        ),
    }
}

fn validate_username(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("用户名长度必须在 1 到 64 个字符之间。".to_string());
    }
    if name.starts_with('-') || name.starts_with('.') {
        return Err("用户名不能以 - 或 . 开头。".to_string());
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
    {
        return Err("用户名只能包含字母、数字、下划线、点号和连字符。".to_string());
    }
    Ok(())
}

fn password_pair(form: &[(String, String)]) -> Result<&str, String> {
    let password = form_value(form, "password").unwrap_or("");
    let confirm = form_value(form, "confirm").unwrap_or("");
    let length = password.chars().count();
    if length < 8 {
        return Err("密码至少需要 8 个字符。".to_string());
    }
    if length > 128 {
        return Err("密码不能超过 128 个字符。".to_string());
    }
    if password.trim().is_empty() {
        return Err("密码不能全是空白字符。".to_string());
    }
    if password.chars().any(char::is_control) {
        return Err("密码不能包含换行或控制字符。".to_string());
    }
    if password != confirm {
        return Err("两次输入的密码不一致。".to_string());
    }
    Ok(password)
}

fn tool_error(label: &str, outcome: &RunOutcome) -> String {
    if outcome.output.trim().is_empty() {
        format!("{label} 失败：{}", outcome.summary())
    } else {
        format!(
            "{label} 失败：{}\n{}",
            outcome.summary(),
            outcome.output.trim()
        )
    }
}

fn create_samba_user(app: &App, form: &[(String, String)]) -> Response {
    let _guard = app.lock_saves();
    let Some(name) = form_value(form, "name").map(str::trim) else {
        return users_response(app, None, Some("请选择 Linux 用户。"), None);
    };
    if let Err(message) = validate_username(name) {
        return users_response(app, None, Some(&message), None);
    }
    let password = match password_pair(form) {
        Ok(password) => password,
        Err(message) => return users_response(app, None, Some(&message), None),
    };
    let Some(pdbedit) = app.pdbedit.as_ref() else {
        return users_response(app, None, Some("未找到 pdbedit，用户管理不可用。"), None);
    };

    let current = match samba_users(app) {
        Ok(current) => current,
        Err(message) => {
            return pages::error_page(
                &app.view(),
                status_code::INTERNAL_SERVER_ERROR,
                "读取 Samba 用户失败",
                &message,
                None,
            )
        }
    };
    if current
        .iter()
        .any(|user| user.name.eq_ignore_ascii_case(name))
    {
        return users_response(app, None, Some("该用户已经有 Samba 账户。"), None);
    }
    let candidates = linux_candidates(&current);
    if !candidates.iter().any(|candidate| candidate.name == name) {
        return users_response(
            app,
            None,
            Some("Linux 用户不存在，或 UID 小于 1000。请先创建 Linux 用户。"),
            None,
        );
    }

    let input = format!("{password}\n{password}\n");
    let outcome = match util::run_program_input(
        "pdbedit -a",
        pdbedit,
        &["-a", "-u", name, "-t"],
        &input,
        Duration::from_secs(20),
    ) {
        Ok(outcome) => outcome,
        Err(message) => return users_response(app, None, Some(&message), None),
    };
    if !outcome.success() {
        return users_response(
            app,
            None,
            Some("创建 Samba 用户失败。"),
            Some(&tool_error("pdbedit", &outcome)),
        );
    }

    util::info(format!("已创建 Samba 用户 {name}"));
    Response::redirect("/users?msg=user_created")
}

fn change_samba_password(app: &App, name: &str, form: &[(String, String)]) -> Response {
    let _guard = app.lock_saves();
    if let Err(message) = validate_username(name) {
        return pages::bad_request(&app.view(), &message);
    }
    let password = match password_pair(form) {
        Ok(password) => password,
        Err(message) => return user_response(app, name, None, Some(&message), None),
    };
    let Some(pdbedit) = app.pdbedit.as_ref() else {
        return user_response(
            app,
            name,
            None,
            Some("未找到 pdbedit，用户管理不可用。"),
            None,
        );
    };
    match samba_users(app) {
        Ok(current) if current.iter().any(|user| user.name == name) => {}
        Ok(_) => return pages::not_found(&app.view()),
        Err(message) => {
            return pages::error_page(
                &app.view(),
                status_code::INTERNAL_SERVER_ERROR,
                "读取 Samba 用户失败",
                &message,
                None,
            )
        }
    }

    let input = format!("{password}\n{password}\n");
    let outcome = match util::run_program_input(
        "pdbedit -r",
        pdbedit,
        &["-r", "-u", name, "-t"],
        &input,
        Duration::from_secs(20),
    ) {
        Ok(outcome) => outcome,
        Err(message) => return user_response(app, name, None, Some(&message), None),
    };
    if !outcome.success() {
        return user_response(
            app,
            name,
            None,
            Some("更新 Samba 密码失败。"),
            Some(&tool_error("pdbedit", &outcome)),
        );
    }

    util::info(format!("已更新 Samba 用户密码: {name}"));
    Response::redirect(&format!("/users/{}?msg=password_changed", url_encode(name)))
}

fn delete_samba_user(app: &App, name: &str) -> Response {
    let _guard = app.lock_saves();
    if let Err(message) = validate_username(name) {
        return pages::bad_request(&app.view(), &message);
    }
    let Some(pdbedit) = app.pdbedit.as_ref() else {
        return user_response(
            app,
            name,
            None,
            Some("未找到 pdbedit，用户管理不可用。"),
            None,
        );
    };
    match samba_users(app) {
        Ok(current) if current.iter().any(|user| user.name == name) => {}
        Ok(_) => return pages::not_found(&app.view()),
        Err(message) => {
            return pages::error_page(
                &app.view(),
                status_code::INTERNAL_SERVER_ERROR,
                "读取 Samba 用户失败",
                &message,
                None,
            )
        }
    }

    let outcome = match util::run_program(
        "pdbedit -x",
        pdbedit,
        &["-x", "-u", name],
        Duration::from_secs(20),
    ) {
        Ok(outcome) => outcome,
        Err(message) => return user_response(app, name, None, Some(&message), None),
    };
    if !outcome.success() {
        return user_response(
            app,
            name,
            None,
            Some("删除 Samba 用户失败。"),
            Some(&tool_error("pdbedit", &outcome)),
        );
    }

    util::info(format!("已删除 Samba 用户 {name}"));
    Response::redirect("/users?msg=user_deleted")
}

fn toggle_samba_user(app: &App, name: &str, form: &[(String, String)]) -> Response {
    let _guard = app.lock_saves();
    if let Err(message) = validate_username(name) {
        return pages::bad_request(&app.view(), &message);
    }
    let action = form_value(form, "action").unwrap_or("");
    let enable = match action {
        "enable" => true,
        "disable" => false,
        _ => return pages::bad_request(&app.view(), "未知的用户状态操作。"),
    };
    let Some(smbpasswd) = app.smbpasswd.as_ref() else {
        return user_response(
            app,
            name,
            None,
            Some("未找到 smbpasswd，用户管理不可用。"),
            None,
        );
    };
    match samba_users(app) {
        Ok(current) if current.iter().any(|user| user.name == name) => {}
        Ok(_) => return pages::not_found(&app.view()),
        Err(message) => {
            return pages::error_page(
                &app.view(),
                status_code::INTERNAL_SERVER_ERROR,
                "读取 Samba 用户失败",
                &message,
                None,
            )
        }
    }

    let args = if enable {
        vec!["-e", name]
    } else {
        vec!["-d", name]
    };
    let outcome = match util::run_program("smbpasswd", smbpasswd, &args, Duration::from_secs(20)) {
        Ok(outcome) => outcome,
        Err(message) => return user_response(app, name, None, Some(&message), None),
    };
    if !outcome.success() {
        return user_response(
            app,
            name,
            None,
            Some(if enable {
                "启用用户失败。"
            } else {
                "禁用用户失败。"
            }),
            Some(&tool_error("smbpasswd", &outcome)),
        );
    }

    util::info(format!(
        "{} Samba 用户 {name}",
        if enable { "已启用" } else { "已禁用" }
    ));
    Response::redirect(&format!(
        "/users/{}?msg={}",
        url_encode(name),
        if enable {
            "user_enabled"
        } else {
            "user_disabled"
        }
    ))
}

fn save_global(app: &App, form: &[(String, String)]) -> Response {
    let view = app.view();
    let Some(rev) = form_value(form, "_rev") else {
        return pages::bad_request(&view, "缺少版本标记，请刷新页面后重试。");
    };

    let _guard = app.lock_saves();
    let loaded = match load(app) {
        Ok(loaded) => loaded,
        Err(response) => return response,
    };
    if loaded.rev != rev {
        return pages::conflict(&app.view());
    }

    let mut doc = loaded.doc.clone();
    doc.ensure_global();
    let edits = collect_edits(form, GLOBAL_FIELDS);
    if let Err(err) = doc.apply_edits("global", &edits) {
        return pages::bad_request(&app.view(), &err);
    }

    match app.store.commit(&doc, &loaded.rev) {
        Ok(info) => after_commit(app, &info, "/global?msg=saved_global"),
        Err(CommitError::Validate(detail)) => Response::html(
            status_code::BAD_REQUEST,
            pages::global_page(
                &app.view(),
                &app.csrf,
                &doc,
                &loaded.rev,
                None,
                Some("Samba 语法校验未通过，文件未修改。请修正后重试。"),
                Some(&detail),
            ),
        ),
        Err(CommitError::Conflict) => pages::conflict(&app.view()),
        Err(CommitError::Io(message)) => pages::error_page(
            &app.view(),
            status_code::INTERNAL_SERVER_ERROR,
            "保存失败",
            &message,
            None,
        ),
    }
}

fn save_share(app: &App, name: &str, form: &[(String, String)]) -> Response {
    let view = app.view();
    let Some(rev) = form_value(form, "_rev") else {
        return pages::bad_request(&view, "缺少版本标记，请刷新页面后重试。");
    };

    let _guard = app.lock_saves();
    let loaded = match load(app) {
        Ok(loaded) => loaded,
        Err(response) => return response,
    };
    if loaded.rev != rev {
        return pages::conflict(&app.view());
    }

    let Some(section) = loaded.doc.find_section(name) else {
        return pages::not_found(&view);
    };
    if section.name.eq_ignore_ascii_case("global") {
        return pages::bad_request(&view, "请使用“全局设置”页面编辑 [global]。");
    }

    let mut doc = loaded.doc.clone();
    let edits = collect_edits(form, SHARE_FIELDS);
    if let Err(err) = doc.apply_edits(&section.name, &edits) {
        return pages::bad_request(&app.view(), &err);
    }

    match app.store.commit(&doc, &loaded.rev) {
        Ok(info) => after_commit(
            app,
            &info,
            &format!("/shares/{}?msg=saved_share", url_encode(&section.name)),
        ),
        Err(CommitError::Validate(detail)) => {
            let page = pages::share_page(
                &app.view(),
                &app.csrf,
                &doc,
                &section.name,
                &loaded.rev,
                None,
                Some("Samba 语法校验未通过，文件未修改。请修正后重试。"),
                Some(&detail),
            );
            match page {
                Some(page) => Response::html(status_code::BAD_REQUEST, page),
                None => pages::not_found(&app.view()),
            }
        }
        Err(CommitError::Conflict) => pages::conflict(&app.view()),
        Err(CommitError::Io(message)) => pages::error_page(
            &app.view(),
            status_code::INTERNAL_SERVER_ERROR,
            "保存失败",
            &message,
            None,
        ),
    }
}

fn create_share(app: &App, form: &[(String, String)]) -> Response {
    let view = app.view();
    let Some(rev) = form_value(form, "_rev") else {
        return pages::bad_request(&view, "缺少版本标记，请刷新页面后重试。");
    };

    let _guard = app.lock_saves();
    let loaded = match load(app) {
        Ok(loaded) => loaded,
        Err(response) => return response,
    };
    if loaded.rev != rev {
        return pages::conflict(&app.view());
    }

    let name = form_value(form, "name").unwrap_or("").trim().to_string();
    let error_page = |message: &str, detail: Option<&str>| {
        Response::html(
            status_code::BAD_REQUEST,
            pages::new_share_page(
                &app.view(),
                &app.csrf,
                &loaded.doc,
                &loaded.rev,
                Some(message),
                detail,
                Some(form),
            ),
        )
    };

    if name.is_empty() {
        return error_page("共享名不能为空。", None);
    }
    if name.eq_ignore_ascii_case("global") {
        return error_page("不能创建名为 global 的共享。", None);
    }
    if name.eq_ignore_ascii_case("new") {
        return error_page("共享名 new 是页面保留名称，请换一个。", None);
    }
    if let Err(err) = conf::validate_section_name(&name) {
        return error_page(&err, None);
    }
    if loaded.doc.find_section(&name).is_some() {
        return error_page(&format!("共享 [{name}] 已存在。"), None);
    }

    let params = match collect_new_params(form, NEW_SHARE_FIELDS) {
        Ok(params) => params,
        Err(err) => return error_page(&err, None),
    };
    if !params
        .iter()
        .any(|(key, value)| key.eq_ignore_ascii_case("path") && !value.trim().is_empty())
    {
        return error_page("目录 path 不能为空。", None);
    }

    let mut doc = loaded.doc.clone();
    if let Err(err) = doc.add_section(&name, &params) {
        return error_page(&err, None);
    }

    match app.store.commit(&doc, &loaded.rev) {
        Ok(info) => after_commit(
            app,
            &info,
            &format!("/shares/{}?msg=created_share", url_encode(&name)),
        ),
        Err(CommitError::Validate(detail)) => Response::html(
            status_code::BAD_REQUEST,
            pages::new_share_page(
                &app.view(),
                &app.csrf,
                &loaded.doc,
                &loaded.rev,
                Some("Samba 语法校验未通过，文件未修改。请修正后重试。"),
                Some(&detail),
                Some(form),
            ),
        ),
        Err(CommitError::Conflict) => pages::conflict(&app.view()),
        Err(CommitError::Io(message)) => pages::error_page(
            &app.view(),
            status_code::INTERNAL_SERVER_ERROR,
            "保存失败",
            &message,
            None,
        ),
    }
}

fn delete_share(app: &App, name: &str, form: &[(String, String)]) -> Response {
    let view = app.view();
    if name.eq_ignore_ascii_case("global") {
        return pages::bad_request(&view, "不能通过此页面删除 [global]。");
    }
    let Some(rev) = form_value(form, "_rev") else {
        return pages::bad_request(&view, "缺少版本标记，请刷新页面后重试。");
    };

    let _guard = app.lock_saves();
    let loaded = match load(app) {
        Ok(loaded) => loaded,
        Err(response) => return response,
    };
    if loaded.rev != rev {
        return pages::conflict(&app.view());
    }
    let Some(section) = loaded.doc.find_section(name) else {
        return pages::not_found(&view);
    };

    let mut doc = loaded.doc.clone();
    if !doc.delete_section(&section.name) {
        return pages::not_found(&app.view());
    }

    match app.store.commit(&doc, &loaded.rev) {
        Ok(info) => after_commit(app, &info, "/?msg=deleted_share"),
        Err(CommitError::Validate(detail)) => pages::error_page(
            &app.view(),
            status_code::BAD_REQUEST,
            "删除失败",
            "Samba 语法校验未通过，文件未修改。",
            Some(&detail),
        ),
        Err(CommitError::Conflict) => pages::conflict(&app.view()),
        Err(CommitError::Io(message)) => pages::error_page(
            &app.view(),
            status_code::INTERNAL_SERVER_ERROR,
            "保存失败",
            &message,
            None,
        ),
    }
}

fn save_raw(app: &App, form: &[(String, String)]) -> Response {
    let view = app.view();
    let Some(rev) = form_value(form, "_rev") else {
        return pages::bad_request(&view, "缺少版本标记，请刷新页面后重试。");
    };
    let Some(content) = form_value(form, "content") else {
        return pages::bad_request(&view, "缺少文件内容。");
    };
    if content.contains('\0') {
        return pages::bad_request(&view, "内容不能包含 NUL 字节。");
    }
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");

    let _guard = app.lock_saves();
    let loaded = match load(app) {
        Ok(loaded) => loaded,
        Err(response) => return response,
    };
    if loaded.rev != rev {
        return pages::conflict(&app.view());
    }

    if normalized == loaded.text {
        return Response::redirect("/raw?msg=saved_raw");
    }

    let mut doc = Doc::parse(&normalized);
    doc.set_newline(loaded.doc.newline());

    match app.store.commit(&doc, &loaded.rev) {
        Ok(info) => after_commit(app, &info, "/raw?msg=saved_raw"),
        Err(CommitError::Validate(detail)) => Response::html(
            status_code::BAD_REQUEST,
            pages::raw_page(
                &app.view(),
                &app.csrf,
                &normalized,
                &loaded.rev,
                None,
                Some("Samba 语法校验未通过，文件未修改。请修正后重试。"),
                Some(&detail),
            ),
        ),
        Err(CommitError::Conflict) => pages::conflict(&app.view()),
        Err(CommitError::Io(message)) => pages::error_page(
            &app.view(),
            status_code::INTERNAL_SERVER_ERROR,
            "保存失败",
            &message,
            None,
        ),
    }
}

fn after_commit(app: &App, info: &CommitInfo, location: &str) -> Response {
    log_commit(info);
    if let Some(reload) = &info.reload {
        if !reload.success() {
            let page = pages::reload_failure_page(&app.view(), info, reload);
            return Response::html(status_code::INTERNAL_SERVER_ERROR, page);
        }
    }
    Response::redirect(location)
}

fn log_commit(info: &CommitInfo) {
    if let Some(backup) = &info.backup {
        util::info(format!("已备份配置: {}", backup.display()));
    }
    if let Some(warnings) = &info.warnings {
        let excerpt: Vec<&str> = warnings.lines().take(6).collect();
        util::warn(format!("testparm 提示:\n{}", excerpt.join("\n")));
    }
    if let Some(reload) = &info.reload {
        if reload.success() {
            util::info("配置已重载");
        } else {
            util::warn(format!("重载失败: {}", reload.summary()));
        }
    }
}

fn load(app: &App) -> Result<Loaded, Response> {
    app.store.load().map_err(|message| {
        pages::error_page(
            &app.view(),
            status_code::INTERNAL_SERVER_ERROR,
            "读取配置文件失败",
            &message,
            None,
        )
    })
}

fn flash(request: &Request) -> Option<&str> {
    request
        .query
        .iter()
        .find(|(key, _)| key == "msg")
        .map(|(_, value)| value.as_str())
}

fn form_value<'a>(form: &'a [(String, String)], name: &str) -> Option<&'a str> {
    form.iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn collect_edits(form: &[(String, String)], fields: &[Field]) -> Vec<Edit> {
    let mut edits = Vec::new();

    for (index, field) in fields.iter().enumerate() {
        let Some(value) = form_value(form, &format!("c.{index}")) else {
            continue;
        };
        if value.trim().is_empty() {
            edits.push(Edit::Delete {
                key: field.key.to_string(),
            });
        } else {
            edits.push(Edit::Set {
                key: field.key.to_string(),
                value: value.to_string(),
            });
        }
    }

    let mut others: Vec<(usize, String)> = Vec::new();
    for (name, value) in form {
        if let Some(rest) = name.strip_prefix("k.") {
            if let Ok(index) = rest.parse::<usize>() {
                let key = value.trim();
                if !key.is_empty() {
                    others.push((index, key.to_string()));
                }
            }
        }
    }
    others.sort_by_key(|(index, _)| *index);
    for (index, key) in others {
        let deleted = form_value(form, &format!("d.{index}"))
            .map(|value| value == "1" || value == "on")
            .unwrap_or(false);
        if deleted {
            edits.push(Edit::Delete { key });
        } else {
            let value = form_value(form, &format!("v.{index}"))
                .unwrap_or("")
                .to_string();
            edits.push(Edit::Set { key, value });
        }
    }

    let new_keys: Vec<&str> = form
        .iter()
        .filter(|(name, _)| name == "nk")
        .map(|(_, value)| value.as_str())
        .collect();
    let new_values: Vec<&str> = form
        .iter()
        .filter(|(name, _)| name == "nv")
        .map(|(_, value)| value.as_str())
        .collect();
    for (index, key) in new_keys.iter().enumerate() {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = new_values.get(index).copied().unwrap_or("");
        edits.push(Edit::Set {
            key: key.to_string(),
            value: value.to_string(),
        });
    }

    edits
}

fn collect_new_params(
    form: &[(String, String)],
    fields: &[Field],
) -> Result<Vec<(String, String)>, String> {
    let mut params: Vec<(String, String)> = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let value = form_value(form, &format!("c.{index}")).unwrap_or("");
        if !value.trim().is_empty() {
            params.push((field.key.to_string(), value.to_string()));
        }
    }

    let new_keys: Vec<&str> = form
        .iter()
        .filter(|(name, _)| name == "nk")
        .map(|(_, value)| value.as_str())
        .collect();
    let new_values: Vec<&str> = form
        .iter()
        .filter(|(name, _)| name == "nv")
        .map(|(_, value)| value.as_str())
        .collect();
    for (index, key) in new_keys.iter().enumerate() {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        conf::validate_param_key(key)?;
        let value = new_values.get(index).copied().unwrap_or("");
        conf::validate_param_value(value)?;
        params.push((key.to_string(), value.to_string()));
    }

    let mut deduped: Vec<(String, String)> = Vec::new();
    for (key, value) in params {
        if let Some(existing) = deduped
            .iter_mut()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(&key))
        {
            existing.1 = value;
        } else {
            deduped.push((key, value));
        }
    }
    Ok(deduped)
}

fn csrf_ok(app: &App, form: &[(String, String)]) -> bool {
    form.iter()
        .any(|(name, value)| name == "_csrf" && util::ct_eq(value.as_bytes(), app.csrf.as_bytes()))
}

fn origin_check(app: &App, request: &Request) -> Result<(), String> {
    let host = request.header("host").unwrap_or("").trim();
    let forwarded_host = request
        .header("x-forwarded-host")
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .unwrap_or("");

    if host.is_empty() && forwarded_host.is_empty() {
        return Err("请求缺少 Host 头，无法校验来源。".to_string());
    }

    let (kind, source) = match (request.header("origin"), request.header("referer")) {
        (Some(origin), _) => {
            if origin.trim().eq_ignore_ascii_case("null") {
                // 部分浏览器、隐私插件、沙箱/内嵌预览会对同源表单发送 Origin: null。
                // 这里回退到后续的 CSRF 校验，避免正常保存被误杀。
                return Ok(());
            }
            ("Origin", origin)
        }
        (None, Some(referer)) => ("Referer", referer),
        (None, None) => return Ok(()),
    };

    let authority = match url_authority(source) {
        Some(authority) => authority,
        None => {
            return Err(origin_failure(
                kind,
                source,
                host,
                forwarded_host,
                &app.opts.allow_origins,
            ))
        }
    };

    let matches_expected = (!host.is_empty() && authority_equivalent(authority, host))
        || (!forwarded_host.is_empty() && authority_equivalent(authority, forwarded_host));

    let matches_allowed = app.opts.allow_origins.iter().any(|allowed| {
        let allowed_authority = url_authority(allowed).unwrap_or(allowed.as_str());
        authority_equivalent(authority, allowed_authority)
    });

    if matches_expected || matches_allowed {
        Ok(())
    } else {
        Err(origin_failure(
            kind,
            source,
            host,
            forwarded_host,
            &app.opts.allow_origins,
        ))
    }
}

fn origin_failure(
    kind: &str,
    source: &str,
    host: &str,
    forwarded_host: &str,
    allowed: &[String],
) -> String {
    let allowed_text = if allowed.is_empty() {
        "（未配置）".to_string()
    } else {
        allowed.join(", ")
    };
    let suggestion = url_authority(source)
        .map(|authority| format!("http://{authority}"))
        .unwrap_or_else(|| "http://<浏览器地址>:<端口>".to_string());

    format!(
        "浏览器地址与服务器看到的 Host 不一致。\n\n\
         {kind}: {source}\n\
         Host: {host}\n\
         X-Forwarded-Host: {forwarded_host}\n\
         允许的额外 Origin: {allowed_text}\n\n\
         处理建议：\n\
         1. 反向代理保留原始 Host 和端口（Nginx 用 proxy_set_header Host $http_host;，\
         并建议加 proxy_set_header X-Forwarded-Host $http_host;）。\n\
         2. 或启动时加 --allow-origin {suggestion}（也可用 SAMBA_UI_ALLOW_ORIGINS）。\n\
         3. 或直接用 http://<服务器IP>:<端口>/ 访问。",
        kind = kind,
        source = truncate(source, 300),
        host = if host.is_empty() { "-" } else { host },
        forwarded_host = if forwarded_host.is_empty() {
            "-"
        } else {
            forwarded_host
        },
        allowed_text = allowed_text,
        suggestion = suggestion,
    )
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max).collect();
    out.push('…');
    out
}

fn url_authority(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let rest = match value.find("://") {
        Some(index) => &value[index + 3..],
        None => value,
    };
    let authority = rest
        .split(|character| character == '/' || character == '?' || character == '#')
        .next()
        .unwrap_or("")
        .trim();
    if authority.is_empty() {
        None
    } else {
        Some(authority)
    }
}

fn authority_equivalent(left: &str, right: &str) -> bool {
    match (normalize_authority(left), normalize_authority(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn normalize_authority(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (host, port) = split_authority(value)?;
    let host = host
        .trim_matches(|character| character == '[' || character == ']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    let port = match port {
        Some(80) | Some(443) => None,
        other => other,
    };
    let ipv6 = host.contains(':');
    Some(match port {
        Some(port) if ipv6 => format!("[{host}]:{port}"),
        Some(port) => format!("{host}:{port}"),
        None if ipv6 => format!("[{host}]"),
        None => host,
    })
}

fn split_authority(value: &str) -> Option<(&str, Option<u16>)> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    if let Some(rest) = value.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &value[..end + 2];
        let after = &rest[end + 1..];
        let port = match after.strip_prefix(':') {
            Some(port) if !port.is_empty() => Some(port.parse().ok()?),
            Some(_) => None,
            None if after.is_empty() => None,
            None => return None,
        };
        return Some((host, port));
    }

    if value.matches(':').count() > 1 {
        // 未加方括号的 IPv6 字面量，视为没有端口。
        return Some((value, None));
    }

    match value.split_once(':') {
        Some((host, port)) if !host.is_empty() => {
            let port = if port.is_empty() {
                None
            } else {
                Some(port.parse().ok()?)
            };
            Some((host, port))
        }
        Some(_) => None,
        None => Some((value, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authority_comparison_handles_ports_and_ipv6() {
        assert!(authority_equivalent("nas:8080", "nas:8080"));
        assert!(authority_equivalent("nas:8080", "NAS:8080"));
        assert!(authority_equivalent("nas:80", "nas"));
        assert!(authority_equivalent("nas:443", "nas"));
        assert!(authority_equivalent("nas.", "nas"));
        assert!(authority_equivalent("[::1]:8080", "[::1]:8080"));
        assert!(authority_equivalent("[::1]", "::1"));
        assert!(!authority_equivalent("[::1]:8080", "::1:8080"));
        assert!(!authority_equivalent("nas:8080", "nas:8081"));
        assert!(!authority_equivalent("nas:8080", "127.0.0.1:8080"));
        assert!(!authority_equivalent("nas", ""));
    }

    #[test]
    fn url_authority_extraction() {
        assert_eq!(url_authority("http://nas:8080/global"), Some("nas:8080"));
        assert_eq!(url_authority("https://nas"), Some("nas"));
        assert_eq!(url_authority("http://[::1]:8080/x"), Some("[::1]:8080"));
        assert_eq!(url_authority("http://nas:8080?x=1"), Some("nas:8080"));
        assert_eq!(url_authority(""), None);
    }
}
