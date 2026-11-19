//! 各个页面的 HTML 渲染。

use crate::conf::Doc;
use crate::html::{self, Nav, View};
use crate::http::{status_code, url_encode, Response};
use crate::store::{BackupEntry, CommitInfo};
use crate::users::{Candidate, SambaUser};
use crate::util::{self, RunOutcome};

pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub hint: &'static str,
}

pub enum FieldKind {
    Text(&'static str),
    Choice(&'static [&'static str]),
    Bool,
}

pub const GLOBAL_FIELDS: &[Field] = &[
    Field {
        key: "workgroup",
        label: "工作组 workgroup",
        kind: FieldKind::Text("WORKGROUP"),
        hint: "",
    },
    Field {
        key: "server string",
        label: "服务器描述 server string",
        kind: FieldKind::Text("Samba Server %v"),
        hint: "可包含 %v 等 Samba 变量",
    },
    Field {
        key: "netbios name",
        label: "NetBIOS 名称 netbios name",
        kind: FieldKind::Text("NAS"),
        hint: "",
    },
    Field {
        key: "security",
        label: "认证模式 security",
        kind: FieldKind::Choice(&["user", "share", "server", "ads", "domain"]),
        hint: "现代 Samba 建议使用 user",
    },
    Field {
        key: "map to guest",
        label: "游客映射 map to guest",
        kind: FieldKind::Choice(&["Never", "Bad User", "Bad Password"]),
        hint: "Bad User 允许匿名访问 guest ok 的共享",
    },
    Field {
        key: "interfaces",
        label: "监听网卡 interfaces",
        kind: FieldKind::Text("eth0 192.168.1.0/24"),
        hint: "留空表示由系统决定",
    },
    Field {
        key: "bind interfaces only",
        label: "仅监听指定网卡 bind interfaces only",
        kind: FieldKind::Bool,
        hint: "",
    },
    Field {
        key: "server min protocol",
        label: "最低协议 server min protocol",
        kind: FieldKind::Choice(&["SMB2", "SMB2_02", "SMB2_10", "SMB3", "NT1"]),
        hint: "老设备可能需要 NT1（不安全）",
    },
    Field {
        key: "server max protocol",
        label: "最高协议 server max protocol",
        kind: FieldKind::Choice(&["SMB2", "SMB3", "SMB3_11", "NT1"]),
        hint: "",
    },
    Field {
        key: "log file",
        label: "日志文件 log file",
        kind: FieldKind::Text("/var/log/samba/log.%m"),
        hint: "",
    },
    Field {
        key: "max log size",
        label: "日志大小上限 max log size",
        kind: FieldKind::Text("1000"),
        hint: "单位 KB",
    },
    Field {
        key: "log level",
        label: "日志级别 log level",
        kind: FieldKind::Text("1"),
        hint: "",
    },
    Field {
        key: "unix charset",
        label: "字符集 unix charset",
        kind: FieldKind::Text("UTF-8"),
        hint: "",
    },
    Field {
        key: "load printers",
        label: "加载打印机 load printers",
        kind: FieldKind::Bool,
        hint: "",
    },
    Field {
        key: "disable spoolss",
        label: "禁用打印服务 disable spoolss",
        kind: FieldKind::Bool,
        hint: "",
    },
    Field {
        key: "usershare allow guests",
        label: "允许用户共享给游客 usershare allow guests",
        kind: FieldKind::Bool,
        hint: "",
    },
];

pub const SHARE_FIELDS: &[Field] = &[
    Field {
        key: "comment",
        label: "说明 comment",
        kind: FieldKind::Text("公共目录"),
        hint: "",
    },
    Field {
        key: "path",
        label: "目录 path",
        kind: FieldKind::Text("/srv/samba/share"),
        hint: "服务器上的绝对路径，必须存在",
    },
    Field {
        key: "browseable",
        label: "在网络上可见 browseable",
        kind: FieldKind::Bool,
        hint: "",
    },
    Field {
        key: "read only",
        label: "只读 read only",
        kind: FieldKind::Bool,
        hint: "设为 no 表示可写",
    },
    Field {
        key: "guest ok",
        label: "允许游客 guest ok",
        kind: FieldKind::Bool,
        hint: "需要配合 map to guest",
    },
    Field {
        key: "valid users",
        label: "允许的用户 valid users",
        kind: FieldKind::Text("alice,bob 或 %S"),
        hint: "逗号分隔；%S 表示当前用户",
    },
    Field {
        key: "write list",
        label: "可写用户 write list",
        kind: FieldKind::Text("alice"),
        hint: "",
    },
    Field {
        key: "force user",
        label: "强制用户 force user",
        kind: FieldKind::Text("nobody"),
        hint: "",
    },
    Field {
        key: "force group",
        label: "强制组 force group",
        kind: FieldKind::Text("users"),
        hint: "",
    },
    Field {
        key: "create mask",
        label: "新文件权限 create mask",
        kind: FieldKind::Text("0664"),
        hint: "八进制",
    },
    Field {
        key: "directory mask",
        label: "新目录权限 directory mask",
        kind: FieldKind::Text("0775"),
        hint: "八进制",
    },
    Field {
        key: "printable",
        label: "打印共享 printable",
        kind: FieldKind::Bool,
        hint: "",
    },
    Field {
        key: "follow symlinks",
        label: "跟随符号链接 follow symlinks",
        kind: FieldKind::Bool,
        hint: "",
    },
    Field {
        key: "veto files",
        label: "排除文件 veto files",
        kind: FieldKind::Text(".DS_Store/Thumbs.db"),
        hint: "以 / 分隔",
    },
];

pub const NEW_SHARE_FIELDS: &[Field] = &[
    Field {
        key: "comment",
        label: "说明 comment",
        kind: FieldKind::Text("我的共享"),
        hint: "",
    },
    Field {
        key: "path",
        label: "目录 path（必填）",
        kind: FieldKind::Text("/srv/samba/share"),
        hint: "服务器绝对路径",
    },
    Field {
        key: "browseable",
        label: "在网络上可见 browseable",
        kind: FieldKind::Bool,
        hint: "",
    },
    Field {
        key: "read only",
        label: "只读 read only",
        kind: FieldKind::Bool,
        hint: "",
    },
    Field {
        key: "guest ok",
        label: "允许游客 guest ok",
        kind: FieldKind::Bool,
        hint: "",
    },
    Field {
        key: "valid users",
        label: "允许的用户 valid users",
        kind: FieldKind::Text("alice,bob"),
        hint: "",
    },
    Field {
        key: "create mask",
        label: "新文件权限 create mask",
        kind: FieldKind::Text("0664"),
        hint: "",
    },
    Field {
        key: "directory mask",
        label: "新目录权限 directory mask",
        kind: FieldKind::Text("0775"),
        hint: "",
    },
];

fn control(field: &Field, name: &str, value: &str) -> String {
    match &field.kind {
        FieldKind::Text(placeholder) => html::text_input(name, value, placeholder),
        FieldKind::Choice(options) => html::select_input(name, value, options, "（未设置）"),
        FieldKind::Bool => html::select_input(name, value, &["yes", "no"], "（未设置）"),
    }
}

pub fn curated_fields(fields: &[Field], doc: &Doc, section: &str) -> String {
    let mut out = String::new();
    for (index, field) in fields.iter().enumerate() {
        let name = format!("c.{index}");
        let value = doc.get(section, field.key).unwrap_or_default();
        out.push_str(&html::field(
            field.label,
            &control(field, &name, &value),
            field.hint,
        ));
    }
    out
}

fn is_curated(key: &str, fields: &[Field]) -> bool {
    fields
        .iter()
        .any(|field| field.key.eq_ignore_ascii_case(key))
}

fn other_params(doc: &Doc, section: Option<&crate::conf::Section>, fields: &[Field]) -> String {
    let Some(section) = section else {
        return "<p class=\"muted\">保存时会自动创建 [global] 段。</p>".to_string();
    };
    let mut rows = String::new();
    for param in doc.params(section) {
        if is_curated(&param.key, fields) {
            continue;
        }
        rows.push_str(&format!(
            "<tr>\
               <td class=\"mono\">{key}{hidden}</td>\
               <td><input type=\"text\" name=\"v.{index}\" value=\"{value}\" \
                    autocomplete=\"off\" spellcheck=\"false\"></td>\
               <td class=\"center\"><input type=\"checkbox\" name=\"d.{index}\" value=\"1\" \
                    aria-label=\"删除 {key}\"></td>\
             </tr>",
            key = html::escape(&param.key),
            hidden = html::hidden(&format!("k.{}", param.index), &param.key),
            index = param.index,
            value = html::escape(&param.value),
        ));
    }
    if rows.is_empty() {
        return "<p class=\"muted\">没有其他参数。</p>".to_string();
    }
    format!(
        "<div class=\"table-wrap\"><table class=\"params\"><thead><tr>\
           <th>参数</th><th>值</th><th class=\"center\">删除</th>\
         </tr></thead><tbody>{rows}</tbody></table></div>\
         <p class=\"muted\">删除勾选后点击保存生效；未勾选的行即使值留空也会写成空值。</p>"
    )
}

fn new_param_rows() -> String {
    let row = "<div class=\"newrow\">\
        <input type=\"text\" name=\"nk\" placeholder=\"参数名，如 wins support\" \
               autocomplete=\"off\" spellcheck=\"false\">\
        <input type=\"text\" name=\"nv\" placeholder=\"值，如 yes\" \
               autocomplete=\"off\" spellcheck=\"false\">\
    </div>";
    format!(
        "<div id=\"new-param-rows\">{row}{row}</div>\
         <template id=\"new-param-template\">{row}</template>\
         <button type=\"button\" class=\"btn\" data-add-row>+ 添加一行</button>"
    )
}

fn submitted<'a>(form: Option<&'a [(String, String)]>, name: &str) -> &'a str {
    form.and_then(|pairs| {
        pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    })
    .unwrap_or("")
}

pub fn dashboard(view: &View, doc: &Doc, size: u64, flash_code: Option<&str>) -> String {
    let mut body = String::new();
    body.push_str("<h1>概览</h1>");
    body.push_str(&html::flash(flash_code));

    for warning in view.warnings {
        body.push_str(&html::banner("warn", warning));
    }
    if doc.global_count() > 1 {
        body.push_str(&html::banner(
            "warn",
            "配置中存在多个 [global] 段：表单只编辑第一个，建议使用原始编辑整理。",
        ));
    }
    if doc.has_preamble_params() {
        body.push_str(&html::banner(
            "warn",
            "第一个配置段之前存在参数行，它们不会被表单修改。",
        ));
    }

    body.push_str("<div class=\"card\"><h2>运行状态</h2><div class=\"stats\">");
    let status_value = |label: &str, value: &str| {
        format!(
            "<div class=\"stat\"><div class=\"stat-label\">{}</div>\
             <div class=\"stat-value\">{}</div></div>",
            html::escape(label),
            value
        )
    };
    body.push_str(&status_value(
        "配置文件",
        &format!("<span class=\"mono\">{}</span>", html::escape(view.conf)),
    ));
    body.push_str(&status_value("文件大小", &format!("{} 字节", size)));
    body.push_str(&status_value("语法校验", &html::escape(view.validator)));
    body.push_str(&status_value("保存后重载", &html::escape(view.reload)));
    body.push_str(&status_value(
        "登录认证",
        if view.auth_enabled {
            "已启用（HTTP Basic）"
        } else {
            "未启用"
        },
    ));
    body.push_str(&status_value(
        "备份",
        &format!(
            "{}（保留 {} 份）",
            html::escape(view.backup_dir),
            view.backup_count
        ),
    ));
    body.push_str("</div></div>");

    body.push_str(
        "<div class=\"card\"><div class=\"card-head\"><h2>全局设置</h2>\
         <a class=\"btn small\" href=\"/global\">编辑全局</a></div>",
    );
    let global_keys = [
        ("工作组", "workgroup"),
        ("服务器描述", "server string"),
        ("认证模式", "security"),
        ("最低协议", "server min protocol"),
    ];
    if doc.global().is_some() {
        body.push_str("<div class=\"table-wrap\"><table><tbody>");
        for (label, key) in global_keys {
            let value = doc
                .get("global", key)
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "（未设置）".to_string());
            body.push_str(&format!(
                "<tr><th class=\"key\">{}</th><td class=\"mono\">{}</td></tr>",
                html::escape(label),
                html::escape(&value)
            ));
        }
        body.push_str("</tbody></table></div>");
    } else {
        body.push_str(&html::banner(
            "warn",
            "当前配置没有 [global] 段，可点击“编辑全局”创建。",
        ));
    }
    body.push_str("</div>");

    body.push_str(
        "<div class=\"card\"><div class=\"card-head\"><h2>共享</h2>\
         <a class=\"btn small primary\" href=\"/shares/new\">+ 新建共享</a></div>",
    );
    let shares = doc.shares();
    if shares.is_empty() {
        body.push_str("<p class=\"muted\">还没有除 [global] 以外的配置段。</p>");
    } else {
        body.push_str(
            "<div class=\"table-wrap\"><table><thead><tr>\
               <th>名称</th><th>目录</th><th>说明</th><th>访问</th><th>操作</th>\
             </tr></thead><tbody>",
        );
        for share in &shares {
            let name = &share.name;
            let path = doc.get(name, "path").unwrap_or_default();
            let comment = doc.get(name, "comment").unwrap_or_default();
            let guest = doc.get(name, "guest ok").unwrap_or_default();
            let printable = doc.get(name, "printable").unwrap_or_default();

            let mut access = Vec::new();
            let read_only_setting = doc.get(name, "read only").filter(|value| !value.is_empty());
            let writable_setting = doc.get(name, "writable").filter(|value| !value.is_empty());
            if let Some(setting) = read_only_setting {
                access.push(if setting.eq_ignore_ascii_case("no") {
                    "可写"
                } else {
                    "只读"
                });
            } else if let Some(setting) = writable_setting {
                access.push(if setting.eq_ignore_ascii_case("yes") {
                    "可写"
                } else {
                    "只读"
                });
            }
            if guest.eq_ignore_ascii_case("yes") {
                access.push("游客");
            }
            if printable.eq_ignore_ascii_case("yes") {
                access.push("打印");
            }
            let access = if access.is_empty() {
                "<span class=\"muted\">默认</span>".to_string()
            } else {
                let mut markup = String::new();
                for flag in &access {
                    markup.push_str("<span class=\"pill\">");
                    markup.push_str(&html::escape(flag));
                    markup.push_str("</span>");
                }
                markup
            };

            body.push_str(&format!(
                "<tr>\
                   <td><a href=\"/shares/{link}\">{name}</a></td>\
                   <td class=\"mono\">{path}</td>\
                   <td>{comment}</td>\
                   <td>{access}</td>\
                   <td><a class=\"btn small\" href=\"/shares/{link}\">编辑</a></td>\
                 </tr>",
                link = url_encode(name),
                name = html::escape(name),
                path = html::escape(&path),
                comment = html::escape(&comment),
                access = access,
            ));
        }
        body.push_str("</tbody></table></div>");
    }
    body.push_str(
        "<p class=\"muted\">提示：直接修改目录或共享名请使用原始编辑；表单会保留注释与未知参数。</p>\
         </div>",
    );

    html::page(view, Nav::Dashboard, "概览", &body)
}

pub fn global_page(
    view: &View,
    csrf: &str,
    doc: &Doc,
    rev: &str,
    flash_code: Option<&str>,
    error: Option<&str>,
    error_detail: Option<&str>,
) -> String {
    let mut body = String::new();
    body.push_str("<h1>全局设置 <span class=\"pill\">[global]</span></h1>");
    body.push_str(&html::flash(flash_code));
    if let Some(error) = error {
        body.push_str(&html::banner("err", error));
    }
    if let Some(detail) = error_detail {
        body.push_str(&html::detail_block("校验输出", detail));
    }
    if view.read_only {
        body.push_str(&html::banner("warn", "只读模式：保存按钮已隐藏。"));
    }
    if doc.global().is_none() {
        body.push_str(&html::banner(
            "warn",
            "当前配置没有 [global] 段，保存时会自动创建。",
        ));
    }

    body.push_str("<form method=\"post\" action=\"/global\">");
    body.push_str(&html::csrf_field(csrf));
    body.push_str(&html::hidden("_rev", rev));

    body.push_str("<div class=\"card\"><h2>常用参数</h2><div class=\"grid\">");
    body.push_str(&curated_fields(GLOBAL_FIELDS, doc, "global"));
    body.push_str(
        "</div><p class=\"muted\">留空表示删除该参数；下拉框会保留无法识别的当前值。</p></div>",
    );

    body.push_str("<div class=\"card\"><h2>其他参数</h2>");
    body.push_str(&other_params(doc, doc.global().as_ref(), GLOBAL_FIELDS));
    body.push_str("</div>");

    body.push_str("<div class=\"card\"><h2>新增参数</h2>");
    body.push_str(&new_param_rows());
    body.push_str("</div>");

    body.push_str("<div class=\"actions\">");
    if !view.read_only {
        body.push_str(&html::submit("保存全局设置"));
    }
    body.push_str(&html::link_button("/", "返回概览"));
    body.push_str("</div></form>");

    html::page(view, Nav::Global, "全局设置", &body)
}

#[allow(clippy::too_many_arguments)]
pub fn share_page(
    view: &View,
    csrf: &str,
    doc: &Doc,
    name: &str,
    rev: &str,
    flash_code: Option<&str>,
    error: Option<&str>,
    error_detail: Option<&str>,
) -> Option<String> {
    let section = doc.find_section(name)?;
    let canonical = section.name.clone();
    let encoded = url_encode(&canonical);

    let mut body = String::new();
    body.push_str(&format!(
        "<h1>共享 <span class=\"pill\">[{name}]</span></h1>",
        name = html::escape(&canonical)
    ));
    body.push_str(&html::flash(flash_code));
    if let Some(error) = error {
        body.push_str(&html::banner("err", error));
    }
    if let Some(detail) = error_detail {
        body.push_str(&html::detail_block("校验输出", detail));
    }
    if view.read_only {
        body.push_str(&html::banner("warn", "只读模式：保存与删除按钮已隐藏。"));
    }

    body.push_str(&format!(
        "<form method=\"post\" action=\"/shares/{encoded}\">"
    ));
    body.push_str(&html::csrf_field(csrf));
    body.push_str(&html::hidden("_rev", rev));

    body.push_str("<div class=\"card\"><h2>常用参数</h2><div class=\"grid\">");
    body.push_str(&curated_fields(SHARE_FIELDS, doc, &canonical));
    body.push_str(
        "</div><p class=\"muted\">留空表示删除该参数；修改共享名请使用原始编辑。</p></div>",
    );

    body.push_str("<div class=\"card\"><h2>其他参数</h2>");
    body.push_str(&other_params(doc, Some(&section), SHARE_FIELDS));
    body.push_str("</div>");

    body.push_str("<div class=\"card\"><h2>新增参数</h2>");
    body.push_str(&new_param_rows());
    body.push_str("</div>");

    body.push_str("<div class=\"actions\">");
    if !view.read_only {
        body.push_str(&html::submit("保存共享设置"));
    }
    body.push_str(&html::link_button("/", "返回概览"));
    body.push_str("</div></form>");

    if !view.read_only {
        body.push_str(&format!(
            "<form class=\"danger-zone\" method=\"post\" action=\"/shares/{encoded}/delete\" \
             data-confirm=\"确定删除共享 [{name}] 吗？这会立即写入 smb.conf，并尝试断开当前客户端连接。\">\
             {csrf}{rev}\
             <button type=\"submit\" class=\"btn danger\">删除此共享</button>\
             </form>",
            encoded = encoded,
            name = html::escape(&canonical),
            csrf = html::csrf_field(csrf),
            rev = html::hidden("_rev", rev),
        ));
    }

    Some(html::page(
        view,
        Nav::Global,
        &format!("共享 {}", canonical),
        &body,
    ))
}

fn path_field(field: &Field, name: &str, value: &str, directories: &[String]) -> String {
    let mut picker = String::from(
        "<select data-dir-target=\"#share-path\" aria-label=\"从常用目录选择\">\
         <option value=\"\">从常用目录选择…</option>",
    );
    let mut datalist = String::from("<datalist id=\"dir-options\">");
    for directory in directories {
        let escaped = html::escape(directory);
        picker.push_str(&format!(
            "<option value=\"{}\">{}</option>",
            escaped, escaped
        ));
        datalist.push_str(&format!("<option value=\"{}\"></option>", escaped));
    }
    picker.push_str("</select>");
    datalist.push_str("</datalist>");

    let control = format!(
        "<input id=\"share-path\" type=\"text\" name=\"{}\" value=\"{}\" \
         placeholder=\"/srv/samba/share\" list=\"dir-options\" \
         autocomplete=\"off\" spellcheck=\"false\">{}",
        html::escape(name),
        html::escape(value),
        picker
    );
    let hint = if directories.is_empty() {
        "没有扫描到常用目录，可以直接输入绝对路径。"
    } else {
        "可从常用目录选择，也可以直接输入绝对路径。目录不存在时会保存并给出提示。"
    };

    format!(
        "<div class=\"field\"><label for=\"share-path\"><span class=\"label-text\">{}</span></label>{}\
         <span class=\"hint\">{}</span>{}</div>",
        html::escape(field.label),
        control,
        html::escape(hint),
        datalist
    )
}

#[allow(clippy::too_many_arguments)]
pub fn new_share_page(
    view: &View,
    csrf: &str,
    doc: &Doc,
    rev: &str,
    error: Option<&str>,
    error_detail: Option<&str>,
    submitted_form: Option<&[(String, String)]>,
    directories: &[String],
) -> String {
    let mut body = String::new();
    body.push_str("<h1>新建共享</h1>");
    if let Some(error) = error {
        body.push_str(&html::banner("err", error));
    }
    if let Some(detail) = error_detail {
        body.push_str(&html::detail_block("校验输出", detail));
    }
    if view.read_only {
        body.push_str(&html::banner("warn", "只读模式：无法创建共享。"));
    }
    if doc.global().is_none() {
        body.push_str(&html::banner(
            "warn",
            "当前配置没有 [global] 段，创建共享前建议先完善全局设置。",
        ));
    }

    body.push_str("<form method=\"post\" action=\"/shares/new\">");
    body.push_str(&html::csrf_field(csrf));
    body.push_str(&html::hidden("_rev", rev));

    body.push_str("<div class=\"card\"><h2>基本信息</h2><div class=\"grid\">");
    body.push_str(&html::field(
        "共享名（不能含 [ ] / \\）",
        &html::text_input("name", submitted(submitted_form, "name"), "例如 documents"),
        "会写成 [documents]",
    ));

    for (index, field) in NEW_SHARE_FIELDS.iter().enumerate() {
        let name = format!("c.{index}");
        let value = submitted(submitted_form, &name);
        if field.key == "path" {
            body.push_str(&path_field(field, &name, value, directories));
        } else {
            body.push_str(&html::field(
                field.label,
                &control(field, &name, value),
                field.hint,
            ));
        }
    }
    body.push_str("</div></div>");

    body.push_str("<div class=\"card\"><h2>新增参数</h2>");
    body.push_str(&new_param_rows());
    body.push_str("</div>");

    body.push_str("<div class=\"actions\">");
    if !view.read_only {
        body.push_str(&html::submit("创建共享"));
    }
    body.push_str(&html::link_button("/", "返回概览"));
    body.push_str("</div></form>");

    html::page(view, Nav::Global, "新建共享", &body)
}

fn backup_time(modified: u64) -> String {
    if modified == 0 {
        return "-".to_string();
    }
    let iso = util::utc_string(modified);
    let fallback = format!("{} UTC", iso.replace('T', " ").trim_end_matches('Z'));
    format!(
        "<time datetime=\"{}\" data-local-time>{}</time>",
        html::escape(&iso),
        html::escape(&fallback)
    )
}

pub fn backups_page(
    view: &View,
    csrf: &str,
    backups: &[BackupEntry],
    rev: &str,
    flash_code: Option<&str>,
    error: Option<&str>,
    error_detail: Option<&str>,
) -> String {
    let mut body = String::new();
    body.push_str("<h1>配置备份</h1>");
    body.push_str(&html::flash(flash_code));
    if let Some(error) = error {
        body.push_str(&html::banner("err", error));
    }
    if let Some(detail) = error_detail {
        body.push_str(&html::detail_block("校验输出", detail));
    }
    if view.read_only {
        body.push_str(&html::banner("warn", "只读模式：不能回退配置。"));
    }

    body.push_str("<div class=\"card\"><h2>备份目录</h2>");
    body.push_str(&format!(
        "<p class=\"mono\">{}</p><p class=\"muted\">共 {} 份备份。回退前会先用当前配置创建一份新备份，\
         然后校验并原子替换 smb.conf。</p></div>",
        html::escape(view.backup_dir),
        backups.len()
    ));

    body.push_str("<div class=\"card\"><h2>可用备份</h2>");
    if backups.is_empty() {
        body.push_str(
            "<p class=\"muted\">还没有备份。每次保存配置成功后会在备份目录生成一份带时间戳的副本。</p>",
        );
    } else {
        body.push_str(
            "<div class=\"table-wrap\"><table><thead><tr>\
               <th>备份文件</th><th>修改时间</th><th>大小</th><th>操作</th>\
             </tr></thead><tbody>",
        );
        for backup in backups {
            let time = backup_time(backup.modified);
            let mut action = format!(
                "<div class=\"row-actions\"><a class=\"btn small\" href=\"/backups/{}\">预览</a>",
                url_encode(&backup.name)
            );
            if !view.read_only {
                action.push_str(&format!(
                    "<form method=\"post\" action=\"/backups/restore\" \
                     data-confirm=\"确定回退到备份 {name} 吗？当前配置会先自动备份。\">\
                     {csrf}{rev}{name_field}\
                     <button type=\"submit\" class=\"btn small\">回退到此版本</button>\
                     </form>",
                    name = html::escape(&backup.name),
                    csrf = html::csrf_field(csrf),
                    rev = html::hidden("_rev", rev),
                    name_field = html::hidden("name", &backup.name),
                ));
            }
            action.push_str("</div>");
            body.push_str(&format!(
                "<tr>\
                   <td class=\"mono\">{name}</td>\
                   <td class=\"mono\">{time}</td>\
                   <td>{size} 字节</td>\
                   <td>{action}</td>\
                 </tr>",
                name = html::escape(&backup.name),
                time = time,
                size = backup.size,
                action = action,
            ));
        }
        body.push_str("</tbody></table></div>");
    }
    body.push_str("</div>");

    html::page(view, Nav::Backups, "配置备份", &body)
}

pub fn backup_page(
    view: &View,
    csrf: &str,
    backup: &BackupEntry,
    content: &str,
    rev: &str,
) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "<h1>备份预览 <span class=\"pill mono\">{}</span></h1>",
        html::escape(&backup.name)
    ));
    body.push_str(&html::banner(
        "warn",
        "这里只是预览备份内容，不会修改当前配置。确认无误后再点击回退。",
    ));
    if view.read_only {
        body.push_str(&html::banner("warn", "只读模式：不能回退配置。"));
    }

    body.push_str("<div class=\"card\"><h2>备份信息</h2><div class=\"table-wrap\"><table><tbody>");
    body.push_str(&format!(
        "<tr><th class=\"key\">备份文件</th><td class=\"mono\">{}</td></tr>",
        html::escape(&backup.name)
    ));
    body.push_str(&format!(
        "<tr><th class=\"key\">修改时间</th><td class=\"mono\">{}</td></tr>",
        backup_time(backup.modified)
    ));
    body.push_str(&format!(
        "<tr><th class=\"key\">大小</th><td>{} 字节</td></tr>",
        backup.size
    ));
    body.push_str(&format!(
        "<tr><th class=\"key\">行数</th><td>{} 行</td></tr>",
        content.lines().count()
    ));
    body.push_str("</tbody></table></div></div>");

    body.push_str("<div class=\"card\"><h2>文件内容</h2>");
    body.push_str(&format!(
        "<pre class=\"detail backup-preview\">{}</pre>",
        html::escape(content)
    ));
    body.push_str("</div>");

    body.push_str("<div class=\"actions\">");
    if !view.read_only {
        body.push_str(&format!(
            "<form method=\"post\" action=\"/backups/restore\" \
             data-confirm=\"确定回退到备份 {name} 吗？当前配置会先自动备份。\">\
             {csrf}{rev}{name_field}\
             <button type=\"submit\" class=\"btn primary\">回退到此版本</button>\
             </form>",
            name = html::escape(&backup.name),
            csrf = html::csrf_field(csrf),
            rev = html::hidden("_rev", rev),
            name_field = html::hidden("name", &backup.name),
        ));
    }
    body.push_str(&html::link_button("/backups", "返回备份列表"));
    body.push_str("</div>");

    html::page(view, Nav::Backups, "备份预览", &body)
}

pub fn users_page(
    view: &View,
    csrf: &str,
    users: &[SambaUser],
    candidates: &[Candidate],
    error: Option<&str>,
    error_detail: Option<&str>,
    flash_code: Option<&str>,
) -> String {
    let mut body = String::new();
    body.push_str("<h1>用户管理</h1>");
    body.push_str(&html::flash(flash_code));
    if let Some(error) = error {
        body.push_str(&html::banner("err", error));
    }
    if let Some(detail) = error_detail {
        body.push_str(&html::detail_block("命令输出", detail));
    }
    if view.read_only {
        body.push_str(&html::banner(
            "warn",
            "只读模式：可以查看用户，但不能创建、改密、禁用或删除。",
        ));
    }
    if !view.users_enabled {
        body.push_str(&html::banner(
            "err",
            "未找到 pdbedit 或 smbpasswd，用户管理不可用。请安装 Samba 工具后重启服务。",
        ));
    }
    body.push_str(&html::banner(
        "warn",
        "Samba 密码会通过当前 HTTP 连接发送。请只在本机、SSH 隧道或 TLS 反向代理下使用，不要在不可信网络中操作。",
    ));

    body.push_str(
        "<div class=\"card\"><div class=\"card-head\"><h2>Samba 用户</h2>\
         <span class=\"pill\">共 ",
    );
    body.push_str(&users.len().to_string());
    body.push_str(" 个</span></div>");

    if users.is_empty() {
        body.push_str("<p class=\"muted\">还没有 Samba 用户。</p>");
    } else {
        body.push_str(
            "<div class=\"table-wrap\"><table><thead><tr>\
               <th>用户名</th><th>UID</th><th>全名</th><th>状态</th><th>操作</th>\
             </tr></thead><tbody>",
        );
        for user in users {
            let encoded = url_encode(&user.name);
            let status = if user.enabled {
                "<span class=\"pill\">已启用</span>"
            } else {
                "<span class=\"pill warn\">已禁用</span>"
            };
            let uid = user
                .uid
                .map(|uid| uid.to_string())
                .unwrap_or_else(|| "-".to_string());
            body.push_str(&format!(
                "<tr>\
                   <td><a href=\"/users/{encoded}\" class=\"mono\">{name}</a></td>\
                   <td class=\"mono\">{uid}</td>\
                   <td>{full_name}</td>\
                   <td>{status}</td>\
                   <td><a class=\"btn small\" href=\"/users/{encoded}\">管理</a></td>\
                 </tr>",
                encoded = encoded,
                name = html::escape(&user.name),
                uid = html::escape(&uid),
                full_name = html::escape(&user.full_name),
                status = status,
            ));
        }
        body.push_str("</tbody></table></div>");
    }
    body.push_str("</div>");

    body.push_str("<div class=\"card\"><h2>创建 Samba 用户</h2>");
    if !view.users_enabled {
        body.push_str("<p class=\"muted\">需要先安装并提供 pdbedit、smbpasswd。</p>");
    } else if view.read_only {
        body.push_str("<p class=\"muted\">只读模式下不能创建用户。</p>");
    } else if candidates.is_empty() {
        body.push_str(
            "<p class=\"muted\">没有可用的 Linux 用户。Samba 用户必须对应一个已存在的 Linux 用户，\
             请先执行 <code>sudo useradd -M -s /usr/sbin/nologin 用户名</code>，再刷新本页。<br>\
             <code> -M 不创建主目录，nologin 禁止登录 shell；Samba 文件访问不依赖登录 shell。</code></p>",
        );
    } else {
        body.push_str(
            "<p class=\"muted\">只创建 Samba 账户和密码，不会创建 Linux 用户，也不会修改 Linux 密码。\
             只列出 UID ≥ 1000 且还没有 Samba 账户的用户。</p>",
        );
        body.push_str("<form method=\"post\" action=\"/users/new\">");
        body.push_str(&html::csrf_field(csrf));
        body.push_str("<div class=\"grid\">");

        let names: Vec<&str> = candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect();
        body.push_str(&html::field(
            "Linux 用户",
            &html::select_input("name", "", &names, "（选择用户）"),
            "不会创建 Linux 用户",
        ));
        body.push_str(&html::field(
            "Samba 密码",
            &html::password_input("password", "至少 8 位"),
            "",
        ));
        body.push_str(&html::field(
            "确认密码",
            &html::password_input("confirm", "再次输入"),
            "",
        ));
        body.push_str("</div><div class=\"actions\">");
        body.push_str(&html::submit("创建 Samba 用户"));
        body.push_str(&html::link_button("/users", "刷新列表"));
        body.push_str("</div></form>");
    }
    body.push_str("</div>");

    html::page(view, Nav::Users, "用户管理", &body)
}

pub fn user_page(
    view: &View,
    csrf: &str,
    user: &SambaUser,
    flash_code: Option<&str>,
    error: Option<&str>,
    error_detail: Option<&str>,
) -> String {
    let encoded = url_encode(&user.name);
    let mut body = String::new();
    body.push_str(&format!(
        "<h1>用户 <span class=\"pill mono\">{}</span></h1>",
        html::escape(&user.name)
    ));
    body.push_str(&html::flash(flash_code));
    if let Some(error) = error {
        body.push_str(&html::banner("err", error));
    }
    if let Some(detail) = error_detail {
        body.push_str(&html::detail_block("命令输出", detail));
    }
    if view.read_only {
        body.push_str(&html::banner(
            "warn",
            "只读模式：不能修改密码、启停或删除用户。",
        ));
    }

    body.push_str("<div class=\"card\"><h2>账户信息</h2><div class=\"table-wrap\"><table><tbody>");
    body.push_str(&format!(
        "<tr><th class=\"key\">用户名</th><td class=\"mono\">{}</td></tr>",
        html::escape(&user.name)
    ));
    body.push_str(&format!(
        "<tr><th class=\"key\">UID</th><td class=\"mono\">{}</td></tr>",
        user.uid
            .map(|uid| uid.to_string())
            .unwrap_or_else(|| "-".to_string())
    ));
    body.push_str(&format!(
        "<tr><th class=\"key\">全名</th><td>{}</td></tr>",
        html::escape(&user.full_name)
    ));
    body.push_str(&format!(
        "<tr><th class=\"key\">状态</th><td>{}</td></tr>",
        if user.enabled {
            "<span class=\"pill\">已启用</span>"
        } else {
            "<span class=\"pill warn\">已禁用</span>"
        }
    ));
    body.push_str("</tbody></table></div></div>");

    if !view.read_only && view.users_enabled {
        body.push_str("<div class=\"card\"><h2>修改 Samba 密码</h2>");
        body.push_str(&format!(
            "<form method=\"post\" action=\"/users/{encoded}/password\">"
        ));
        body.push_str(&html::csrf_field(csrf));
        body.push_str("<div class=\"grid\">");
        body.push_str(&html::field(
            "新密码",
            &html::password_input("password", "至少 8 位"),
            "",
        ));
        body.push_str(&html::field(
            "确认新密码",
            &html::password_input("confirm", "再次输入"),
            "",
        ));
        body.push_str("</div><div class=\"actions\">");
        body.push_str(&html::submit("更新密码"));
        body.push_str("</div></form></div>");

        body.push_str("<div class=\"card\"><h2>账户状态</h2>");
        body.push_str(&format!(
            "<form method=\"post\" action=\"/users/{encoded}/toggle\">"
        ));
        body.push_str(&html::csrf_field(csrf));
        body.push_str(&html::hidden(
            "action",
            if user.enabled { "disable" } else { "enable" },
        ));
        body.push_str(&format!(
            "<button type=\"submit\" class=\"btn\">{}</button>",
            if user.enabled {
                "禁用此用户"
            } else {
                "启用此用户"
            }
        ));
        body.push_str("</form></div>");

        body.push_str(&format!(
            "<form class=\"danger-zone\" method=\"post\" action=\"/users/{encoded}/delete\" \
             data-confirm=\"确定删除 Samba 用户 [{name}] 吗？Linux 用户不会被删除。\">\
             {csrf}<button type=\"submit\" class=\"btn danger\">删除 Samba 用户</button>\
             </form>",
            encoded = encoded,
            name = html::escape(&user.name),
            csrf = html::csrf_field(csrf),
        ));
    }

    body.push_str("<div class=\"actions\">");
    body.push_str(&html::link_button("/users", "返回用户列表"));
    body.push_str("</div>");
    html::page(view, Nav::Users, &format!("用户 {}", user.name), &body)
}

pub fn raw_page(
    view: &View,
    csrf: &str,
    text: &str,
    rev: &str,
    flash_code: Option<&str>,
    error: Option<&str>,
    error_detail: Option<&str>,
) -> String {
    let mut body = String::new();
    body.push_str("<h1>原始编辑</h1>");
    body.push_str(&html::flash(flash_code));
    if let Some(error) = error {
        body.push_str(&html::banner("err", error));
    }
    if let Some(detail) = error_detail {
        body.push_str(&html::detail_block("校验输出", detail));
    }
    body.push_str(&html::banner(
        "warn",
        "这里直接编辑整个 smb.conf。保存前会做语法校验，并通过后原子替换文件。",
    ));
    body.push_str(&format!(
        "<p class=\"muted\">{} 行 · {} 字节 · 保存后保留原有换行风格</p>",
        text.lines().count(),
        text.len()
    ));

    if view.read_only {
        body.push_str(&format!(
            "<div class=\"card\"><textarea readonly spellcheck=\"false\">{}</textarea></div>",
            html::escape(text)
        ));
    } else {
        body.push_str(&format!(
            "<form method=\"post\" action=\"/raw\" \
             data-confirm=\"确定用当前内容覆盖整个 smb.conf 吗？\">\
             {csrf}{rev}\
             <div class=\"card\"><textarea name=\"content\" spellcheck=\"false\" \
                aria-label=\"smb.conf 内容\">{content}</textarea></div>\
             <div class=\"actions\">{save}{back}</div></form>",
            csrf = html::csrf_field(csrf),
            rev = html::hidden("_rev", rev),
            content = html::escape(text),
            save = html::submit("保存整个文件"),
            back = html::link_button("/", "返回概览"),
        ));
    }

    html::page(view, Nav::Raw, "原始编辑", &body)
}

pub fn reload_failure_page(view: &View, info: &CommitInfo, outcome: &RunOutcome) -> String {
    let mut body = String::new();
    body.push_str("<h1>配置已写入，但重载失败</h1>");
    body.push_str(&html::banner(
        "err",
        "文件已保存并保留备份，但自动重载命令执行失败。正在运行的 Samba 可能仍在使用旧配置。",
    ));
    body.push_str("<div class=\"card\"><h2>执行结果</h2>");
    body.push_str(&format!(
        "<p>状态：<span class=\"mono\">{}</span></p>",
        html::escape(&outcome.summary())
    ));
    body.push_str(&html::detail_block("命令输出", &outcome.output));
    body.push_str("</div>");

    body.push_str("<div class=\"card\"><h2>下一步</h2><ul>");
    if let Some(backup) = &info.backup {
        body.push_str(&format!(
            "<li>备份文件：<span class=\"mono\">{}</span></li>",
            html::escape(&backup.display().to_string())
        ));
    }
    body.push_str(
        "<li>检查 <code>smbcontrol</code> 是否可用，或手动执行 <code>smbcontrol all reload-config</code>。</li>\
         <li>如果新配置导致 Samba 异常，可用备份文件恢复后再次重载。</li>\
         </ul></div>",
    );
    body.push_str("<div class=\"actions\"><a class=\"btn\" href=\"/\">返回概览</a></div>");
    html::page(view, Nav::None, "重载失败", &body)
}

pub fn reload_manual_failure_page(view: &View, outcome: &RunOutcome) -> String {
    let mut body = String::new();
    body.push_str("<h1>重载失败</h1>");
    body.push_str(&html::banner(
        "err",
        "Samba 重新加载失败，配置文件没有被修改。正在运行的 Samba 可能仍在使用旧配置。",
    ));
    body.push_str("<div class=\"card\"><h2>执行结果</h2>");
    body.push_str(&format!(
        "<p>状态：<span class=\"mono\">{}</span></p>",
        html::escape(&outcome.summary())
    ));
    body.push_str(&html::detail_block("命令输出", &outcome.output));
    body.push_str("</div>");

    body.push_str(
        "<div class=\"card\"><h2>下一步</h2><ul>\
         <li>在服务器上执行 <code>sudo testparm -s</code> 检查配置。</li>\
         <li>手动重载：<code>sudo smbcontrol all reload-config</code>。</li>\
         <li>如果仍失败，检查 Samba 服务状态和重载命令配置。</li>\
         </ul></div>",
    );
    body.push_str("<div class=\"actions\"><a class=\"btn\" href=\"/\">返回概览</a></div>");
    html::page(view, Nav::None, "重载失败", &body)
}

pub fn error_page(
    view: &View,
    status: u16,
    title: &str,
    message: &str,
    detail: Option<&str>,
) -> Response {
    let mut body = format!(
        "<h1>{}</h1>{}",
        html::escape(title),
        html::banner("err", message)
    );
    if let Some(detail) = detail {
        body.push_str(&html::detail_block("详细信息", detail));
    }
    body.push_str("<div class=\"actions\"><a class=\"btn\" href=\"/\">返回概览</a></div>");
    Response::html(status, html::page(view, Nav::None, title, &body))
}

pub fn unauthorized(view: &View) -> Response {
    let body = "<h1>需要登录</h1>\
        <p class=\"muted\">此页面启用了 HTTP Basic 认证，请输入用户名与口令。</p>";
    Response::html(
        status_code::UNAUTHORIZED,
        html::page(view, Nav::None, "需要登录", body),
    )
    .with_header(
        "WWW-Authenticate",
        "Basic realm=\"samba-ui\", charset=\"UTF-8\"",
    )
}

pub fn forbidden(view: &View, message: &str) -> Response {
    error_page(view, status_code::FORBIDDEN, "请求被拒绝", message, None)
}

pub fn not_found(view: &View) -> Response {
    error_page(
        view,
        status_code::NOT_FOUND,
        "页面不存在",
        "请求的页面或共享不存在。",
        None,
    )
}

pub fn bad_request(view: &View, message: &str) -> Response {
    error_page(view, status_code::BAD_REQUEST, "请求无效", message, None)
}

pub fn conflict(view: &View) -> Response {
    error_page(
        view,
        status_code::CONFLICT,
        "配置已被外部修改",
        "在你编辑期间，smb.conf 已被其他程序修改。为避免覆盖，请返回并刷新后重试。",
        None,
    )
}

pub fn method_not_allowed(view: &View) -> Response {
    error_page(
        view,
        status_code::METHOD_NOT_ALLOWED,
        "方法不被支持",
        "此服务只接受 GET / HEAD / POST 请求。",
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conf::Doc;

    fn view() -> (String, String, Vec<String>) {
        (
            "/etc/samba/smb.conf".to_string(),
            "testparm".to_string(),
            Vec::new(),
        )
    }

    #[test]
    fn dashboard_escapes_share_names() {
        let (conf, validator, warnings) = view();
        let view = View::new(
            &conf,
            false,
            false,
            &validator,
            "smbcontrol all reload-config",
            "/etc/samba/backups",
            2,
            &warnings,
        )
        .with_csrf("token-abc");
        let doc = Doc::parse("[global]\n   workgroup = W\n\n[<script>]\n   path = /x\n");
        let html = dashboard(&view, &doc, 42, None);
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("[<script>]"));
        assert!(html.contains('W'));
        assert!(html.contains("smbcontrol all reload-config"));
        assert!(html.contains("action=\"/reload\""));
        assert!(html.contains("token-abc"));
        assert!(html.contains("Powered by"));
        assert!(html.contains("https://github.com/JsBeta/samba-web-conf"));
    }

    #[test]
    fn global_page_contains_curated_values_and_csrf() {
        let (conf, validator, warnings) = view();
        let view = View::new(
            &conf, true, false, &validator, "reload", "/backups", 0, &warnings,
        );
        let doc = Doc::parse("[global]\n   workgroup = HOME\n   security = user\n");
        let page = global_page(&view, "token123", &doc, "1-2", None, Some("坏了"), None);
        assert!(page.contains("value=\"HOME\""));
        assert!(page.contains("token123"));
        assert!(page.contains("name=\"_rev\" value=\"1-2\""));
        assert!(page.contains("坏了"));
        assert!(page.contains("action=\"/global\""));
    }

    #[test]
    fn raw_page_escapes_content() {
        let (conf, validator, warnings) = view();
        let view = View::new(
            &conf, false, false, &validator, "reload", "/backups", 0, &warnings,
        );
        let page = raw_page(
            &view,
            "t",
            "[global]\n   comment = <x>\n",
            "1-2",
            None,
            None,
            None,
        );
        assert!(page.contains("&lt;x&gt;"));
        assert!(!page.contains("comment = <x>"));
    }

    #[test]
    fn users_page_escapes_names_and_renders_forms() {
        let (conf, validator, warnings) = view();
        let view = View::new(
            &conf,
            false,
            false,
            &validator,
            "smbcontrol all reload-config",
            "/backups",
            0,
            &warnings,
        )
        .with_csrf("token")
        .with_users(true);
        let users = vec![SambaUser {
            name: "a<b".to_string(),
            uid: Some(1000),
            full_name: "<script>".to_string(),
            enabled: false,
        }];
        let candidates = vec![Candidate {
            name: "bob".to_string(),
            uid: 1001,
            full_name: "Bob".to_string(),
        }];
        let page = users_page(&view, "token", &users, &candidates, None, None, None);
        assert!(page.contains("a&lt;b"));
        assert!(page.contains("&lt;script&gt;"));
        assert!(page.contains("action=\"/users/new\""));
        assert!(page.contains("name=\"password\""));
        assert!(page.contains("已禁用"));
    }

    #[test]
    fn user_page_renders_password_and_delete_forms() {
        let (conf, validator, warnings) = view();
        let view = View::new(
            &conf,
            false,
            false,
            &validator,
            "smbcontrol all reload-config",
            "/backups",
            0,
            &warnings,
        )
        .with_csrf("token")
        .with_users(true);
        let user = SambaUser {
            name: "admin".to_string(),
            uid: Some(1000),
            full_name: "Admin".to_string(),
            enabled: true,
        };
        let page = user_page(&view, "token", &user, None, None, None);
        assert!(page.contains("action=\"/users/admin/password\""));
        assert!(page.contains("action=\"/users/admin/toggle\""));
        assert!(page.contains("action=\"/users/admin/delete\""));
    }

    #[test]
    fn backups_page_lists_restore_form() {
        let (conf, validator, warnings) = view();
        let view = View::new(
            &conf,
            false,
            false,
            &validator,
            "smbcontrol all reload-config",
            "/backups",
            2,
            &warnings,
        )
        .with_csrf("token");
        let backups = vec![BackupEntry {
            name: "smb.conf.20261117-032559-000.bak".to_string(),
            size: 128,
            modified: 1_700_000_000,
        }];
        let page = backups_page(&view, "token", &backups, "1-2", None, None, None);
        assert!(page.contains("action=\"/backups/restore\""));
        assert!(page.contains("name=\"name\" value=\"smb.conf.20261117-032559-000.bak\""));
        assert!(page.contains("回退到此版本"));
        assert!(page.contains("预览"));
        assert!(page.contains("data-local-time"));
        assert!(page.contains(" UTC</time>"));
    }

    #[test]
    fn backup_page_escapes_content() {
        let (conf, validator, warnings) = view();
        let view = View::new(
            &conf,
            false,
            false,
            &validator,
            "smbcontrol all reload-config",
            "/backups",
            1,
            &warnings,
        )
        .with_csrf("token");
        let backup = BackupEntry {
            name: "smb.conf.20261117-032559-000.bak".to_string(),
            size: 32,
            modified: 1_700_000_000,
        };
        let page = backup_page(
            &view,
            "token",
            &backup,
            "[global]\n   comment = <script>\n",
            "1-2",
        );
        assert!(page.contains("&lt;script&gt;"));
        assert!(page.contains("回退到此版本"));
        assert!(page.contains("action=\"/backups/restore\""));
    }

    #[test]
    fn error_page_status_and_detail() {
        let (conf, validator, warnings) = view();
        let view = View::new(
            &conf, false, false, &validator, "reload", "/backups", 0, &warnings,
        );
        let response = error_page(&view, status_code::CONFLICT, "冲突", "换行", Some("<bad>"));
        assert_eq!(response.status, status_code::CONFLICT);
        let body = String::from_utf8(response.body).unwrap();
        assert!(body.contains("&lt;bad&gt;"));
    }
}
