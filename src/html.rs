//! HTML 页面骨架与可复用的表单控件。

use crate::VERSION;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Dashboard,
    Global,
    Raw,
    None,
}

pub struct View<'a> {
    pub conf: &'a str,
    pub auth_enabled: bool,
    pub read_only: bool,
    pub validator: &'a str,
    pub reload: &'a str,
    pub backup_dir: &'a str,
    pub backup_count: usize,
    pub warnings: &'a [String],
    pub version: &'a str,
    pub csrf: &'a str,
}

impl<'a> View<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        conf: &'a str,
        auth_enabled: bool,
        read_only: bool,
        validator: &'a str,
        reload: &'a str,
        backup_dir: &'a str,
        backup_count: usize,
        warnings: &'a [String],
    ) -> View<'a> {
        View {
            conf,
            auth_enabled,
            read_only,
            validator,
            reload,
            backup_dir,
            backup_count,
            warnings,
            version: VERSION,
            csrf: "",
        }
    }

    pub fn with_csrf(mut self, csrf: &'a str) -> View<'a> {
        self.csrf = csrf;
        self
    }
}

pub fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    for character in input.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

pub fn page(view: &View, active: Nav, title: &str, body: &str) -> String {
    let class = |nav: Nav| {
        if nav == active {
            " class=\"active\""
        } else {
            ""
        }
    };
    let read_only = if view.read_only {
        "<span class=\"pill warn\">只读模式</span>"
    } else {
        ""
    };
    let reload = if view.csrf.is_empty() || view.read_only {
        String::new()
    } else {
        format!(
            "<form class=\"header-form\" method=\"post\" action=\"/reload\" \
             data-confirm=\"确定让 Samba 重新加载当前配置吗？\">\
             {csrf}<button type=\"submit\" class=\"btn small\">重载 Samba</button>\
             </form>",
            csrf = hidden("_csrf", view.csrf)
        )
    };
    let conf = escape(view.conf);
    format!(
        "<!doctype html>\n\
<html lang=\"zh-CN\">\n\
<head>\n\
<meta charset=\"utf-8\">\n\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<meta name=\"color-scheme\" content=\"light dark\">\n\
<title>{title} · Samba 配置面板</title>\n\
<link rel=\"stylesheet\" href=\"/static/app.css\">\n\
</head>\n\
<body>\n\
<header class=\"top\">\n\
  <a class=\"brand\" href=\"/\">Samba 配置面板</a>\n\
  <nav>\n\
    <a href=\"/\"{dash}>概览</a>\n\
    <a href=\"/global\"{global}>全局设置</a>\n\
    <a href=\"/raw\"{raw}>原始编辑</a>\n\
  </nav>\n\
  <span class=\"spacer\"></span>\n\
  {reload}\n\
  <span class=\"pill mono\" title=\"{conf}\">{conf}</span>\n\
  {read_only}\n\
</header>\n\
<main>\n\
{body}\n\
</main>\n\
<footer>samba-ui v{version} · 配置 {conf} · 自动备份 {backup_count} 份 · Powered by <a href=\"https://github.com/JsBeta/samba-web-conf\" target=\"_blank\" rel=\"noopener noreferrer\">samba-web-conf</a></footer>\n\
<script src=\"/static/app.js\" defer></script>\n\
</body>\n\
</html>\n",
        title = escape(title),
        dash = class(Nav::Dashboard),
        global = class(Nav::Global),
        raw = class(Nav::Raw),
        reload = reload,
        conf = conf,
        version = escape(view.version),
        backup_count = view.backup_count,
    )
}

pub fn banner(kind: &str, text: &str) -> String {
    format!(
        "<div class=\"banner {kind}\">{text}</div>",
        kind = kind,
        text = escape(text)
    )
}

pub fn flash_message(code: &str) -> Option<(&'static str, &'static str)> {
    match code {
        "saved_global" => Some(("ok", "全局设置已保存。")),
        "saved_share" => Some(("ok", "共享设置已保存。")),
        "created_share" => Some(("ok", "共享已创建。")),
        "deleted_share" => Some(("ok", "共享已删除。")),
        "saved_raw" => Some(("ok", "配置文件已保存。")),
        "reloaded" => Some(("ok", "Samba 已重新加载最新配置。")),
        _ => None,
    }
}

pub fn flash(code: Option<&str>) -> String {
    match code.and_then(flash_message) {
        Some((kind, text)) => banner(kind, text),
        None => String::new(),
    }
}

pub fn hidden(name: &str, value: &str) -> String {
    format!(
        "<input type=\"hidden\" name=\"{}\" value=\"{}\">",
        escape(name),
        escape(value)
    )
}

pub fn text_input(name: &str, value: &str, placeholder: &str) -> String {
    format!(
        "<input type=\"text\" name=\"{}\" value=\"{}\" placeholder=\"{}\" \
         autocomplete=\"off\" spellcheck=\"false\">",
        escape(name),
        escape(value),
        escape(placeholder)
    )
}

pub fn select_input(name: &str, value: &str, options: &[&str], empty_label: &str) -> String {
    let mut html = format!("<select name=\"{}\">", escape(name));
    html.push_str(&format!(
        "<option value=\"\"{}>{}</option>",
        if value.is_empty() { " selected" } else { "" },
        escape(empty_label)
    ));

    let mut known = false;
    for option in options {
        let selected = value.eq_ignore_ascii_case(option);
        if selected {
            known = true;
        }
        // 选中时沿用配置里的原始大小写，避免仅仅打开页面再保存就改写这一行。
        let option_value = if selected && !value.is_empty() {
            value
        } else {
            option
        };
        html.push_str(&format!(
            "<option value=\"{}\"{}>{}</option>",
            escape(option_value),
            if selected { " selected" } else { "" },
            escape(option)
        ));
    }
    if !value.is_empty() && !known {
        html.push_str(&format!(
            "<option value=\"{}\" selected>{}（当前值）</option>",
            escape(value),
            escape(value)
        ));
    }
    html.push_str("</select>");
    html
}

pub fn field(label: &str, control: &str, hint: &str) -> String {
    let hint = if hint.is_empty() {
        String::new()
    } else {
        format!("<span class=\"hint\">{}</span>", escape(hint))
    };
    format!(
        "<div class=\"field\"><label><span class=\"label-text\">{}</span>{}</label>{}</div>",
        escape(label),
        control,
        hint
    )
}

pub fn detail_block(title: &str, text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    format!(
        "<div class=\"detail-title\">{}</div><pre class=\"detail\">{}</pre>",
        escape(title),
        escape(text)
    )
}

pub fn submit(label: &str) -> String {
    format!(
        "<button type=\"submit\" class=\"btn primary\">{}</button>",
        escape(label)
    )
}

pub fn link_button(href: &str, label: &str) -> String {
    format!(
        "<a class=\"btn\" href=\"{}\">{}</a>",
        escape(href),
        escape(label)
    )
}

pub fn csrf_field(token: &str) -> String {
    hidden("_csrf", token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_covers_html_metacharacters() {
        assert_eq!(
            escape("<a href=\"x\">'&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn select_keeps_current_value_casing() {
        let html = select_input("c.3", "USER", &["user", "share"], "（未设置）");
        assert!(html.contains("value=\"USER\" selected"));
        assert!(html.contains(">user</option>"));
    }

    #[test]
    fn select_appends_unknown_value() {
        let html = select_input("c.3", "custom", &["user", "share"], "（未设置）");
        assert!(html.contains("value=\"custom\" selected"));
        assert!(html.contains("custom（当前值）"));
    }
}
