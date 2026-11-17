//! smb.conf 的极简解析 / 序列化 / 编辑模型。
//!
//! 设计目标：
//! - 未改动的行按原始字节保留（注释、缩进、大小写、空行都不丢）；
//! - 只做最小化修改，避免把整份配置重写成“标准化”的样子；
//! - 对无法识别的行保持原样，不丢信息；
//! - 支持 CRLF、BOM 与是否以换行结尾。

use std::collections::{HashMap, HashSet};

const DEFAULT_INDENT: &str = "    ";

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Blank(String),
    Comment(String),
    Section {
        name: String,
        raw: String,
    },
    Param {
        key: String,
        value: String,
        raw: String,
    },
    Other(String),
}

impl Item {
    fn raw(&self) -> &str {
        match self {
            Item::Blank(s) | Item::Comment(s) | Item::Other(s) => s,
            Item::Section { raw, .. } | Item::Param { raw, .. } => raw,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Section {
    pub name: String,
    /// `[section]` 行在 items 中的下标。
    pub header: usize,
    /// 该段结束位置（下一个 `[section]` 的下标，或 items.len()）。
    pub end: usize,
}

#[derive(Clone, Debug)]
pub struct ParamRef {
    pub index: usize,
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub enum Edit {
    Set { key: String, value: String },
    Delete { key: String },
}

struct NormEdit {
    key: String,
    value: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Doc {
    items: Vec<Item>,
    newline: String,
    trailing_newline: bool,
    bom: bool,
}

impl Doc {
    pub fn parse(text: &str) -> Doc {
        let bom = text.starts_with('\u{feff}');
        let body_all = if bom { &text[3..] } else { text };
        let newline = if body_all.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let trailing_newline = body_all.ends_with('\n');
        let body = if trailing_newline {
            &body_all[..body_all.len() - 1]
        } else {
            body_all
        };

        let mut items = Vec::new();
        if !body.is_empty() {
            for line in body.split('\n') {
                let line = line.strip_suffix('\r').unwrap_or(line);
                items.push(classify(line));
            }
        } else if trailing_newline {
            // 文件只有一个换行符：保留为一个空行。
            items.push(Item::Blank(String::new()));
        }

        Doc {
            items,
            newline: newline.to_string(),
            trailing_newline,
            bom,
        }
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        if self.bom {
            out.push('\u{feff}');
        }
        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                out.push_str(&self.newline);
            }
            out.push_str(item.raw());
        }
        if self.trailing_newline && !self.items.is_empty() {
            out.push_str(&self.newline);
        }
        out
    }

    pub fn newline(&self) -> &str {
        &self.newline
    }

    pub fn set_newline(&mut self, newline: &str) {
        self.newline = if newline == "\r\n" { "\r\n" } else { "\n" }.to_string();
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn sections(&self) -> Vec<Section> {
        let mut out = Vec::new();
        let mut current: Option<(String, usize)> = None;
        for (index, item) in self.items.iter().enumerate() {
            if let Item::Section { name, .. } = item {
                if let Some((prev_name, prev_header)) = current.take() {
                    out.push(Section {
                        name: prev_name,
                        header: prev_header,
                        end: index,
                    });
                }
                current = Some((name.clone(), index));
            }
        }
        if let Some((name, header)) = current {
            out.push(Section {
                name,
                header,
                end: self.items.len(),
            });
        }
        out
    }

    pub fn find_section(&self, name: &str) -> Option<Section> {
        self.sections()
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case(name.trim()))
    }

    pub fn global(&self) -> Option<Section> {
        self.sections()
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case("global"))
    }

    pub fn global_count(&self) -> usize {
        self.sections()
            .iter()
            .filter(|s| s.name.eq_ignore_ascii_case("global"))
            .count()
    }

    /// 除 `[global]` 之外的所有配置段（共享、打印机等）。
    pub fn shares(&self) -> Vec<Section> {
        self.sections()
            .into_iter()
            .filter(|s| !s.name.eq_ignore_ascii_case("global"))
            .collect()
    }

    pub fn params(&self, sec: &Section) -> Vec<ParamRef> {
        let mut out = Vec::new();
        for (offset, item) in self.items[sec.header + 1..sec.end].iter().enumerate() {
            if let Item::Param { key, value, .. } = item {
                out.push(ParamRef {
                    index: sec.header + 1 + offset,
                    key: key.clone(),
                    value: value.clone(),
                });
            }
        }
        out
    }

    pub fn section_param_count(&self, sec: &Section) -> usize {
        self.items[sec.header + 1..sec.end]
            .iter()
            .filter(|item| matches!(item, Item::Param { .. }))
            .count()
    }

    pub fn param_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| matches!(item, Item::Param { .. }))
            .count()
    }

    /// 第一个配置段之前是否还有参数行（这种写法在 smb.conf 中意义不明确）。
    pub fn has_preamble_params(&self) -> bool {
        for item in &self.items {
            match item {
                Item::Section { .. } => return false,
                Item::Param { .. } => return true,
                _ => {}
            }
        }
        false
    }

    /// 返回某个参数在该段中的最后一个值（与 Samba 的“后出现的覆盖先出现的”一致）。
    pub fn get(&self, sec_name: &str, key_name: &str) -> Option<String> {
        let sec = self.find_section(sec_name)?;
        let mut found = None;
        for item in &self.items[sec.header + 1..sec.end] {
            if let Item::Param { key, value, .. } = item {
                if key.eq_ignore_ascii_case(key_name.trim()) {
                    found = Some(value.clone());
                }
            }
        }
        found
    }

    pub fn ensure_global(&mut self) {
        if self.global().is_some() {
            return;
        }
        let header = Item::Section {
            name: "global".to_string(),
            raw: "[global]".to_string(),
        };
        if self.items.is_empty() {
            self.items.push(header);
        } else {
            self.items.insert(0, header);
            self.items.insert(1, Item::Blank(String::new()));
        }
        self.trailing_newline = true;
    }

    pub fn add_section(&mut self, name: &str, params: &[(String, String)]) -> Result<(), String> {
        let name = name.trim();
        validate_section_name(name)?;
        if self.find_section(name).is_some() {
            return Err(format!("配置段 [{name}] 已存在"));
        }
        if !self.items.is_empty() && !matches!(self.items.last(), Some(Item::Blank(_))) {
            self.items.push(Item::Blank(String::new()));
        }
        let indent = section_indent(&self.items);
        self.items.push(Item::Section {
            name: name.to_string(),
            raw: format!("[{name}]"),
        });
        for (key, value) in params {
            validate_param_key(key)?;
            validate_param_value(value)?;
            self.items.push(Item::Param {
                key: key.clone(),
                value: value.clone(),
                raw: format!("{indent}{key} = {}", render_value(value)),
            });
        }
        self.trailing_newline = true;
        Ok(())
    }

    pub fn delete_section(&mut self, name: &str) -> bool {
        let Some(sec) = self.find_section(name) else {
            return false;
        };

        // 保留段尾的空行作为分隔符，只删除段本身的头部与参数行。
        let mut cut_end = sec.end;
        while cut_end > sec.header + 1 && matches!(self.items[cut_end - 1], Item::Blank(_)) {
            cut_end -= 1;
        }
        self.items.drain(sec.header..cut_end);

        // 删除后若出现两个连续空行，合并为一个。
        if sec.header > 0
            && sec.header < self.items.len()
            && matches!(self.items[sec.header - 1], Item::Blank(_))
            && matches!(self.items[sec.header], Item::Blank(_))
        {
            self.items.remove(sec.header - 1);
        }
        true
    }

    /// 对某个段应用一组编辑。
    ///
    /// - 值未变化的参数行原样保留；
    /// - 修改了值的参数只重写该行（保留缩进与参数名大小写）；
    /// - 删除参数会删除该段内所有同名行；
    /// - 新参数追加到该段末尾。
    pub fn apply_edits(&mut self, sec_name: &str, edits: &[Edit]) -> Result<(), String> {
        let sec = self
            .find_section(sec_name)
            .ok_or_else(|| format!("找不到配置段 [{sec_name}]"))?;
        let edits = normalize_edits(edits)?;
        if edits.is_empty() {
            return Ok(());
        }

        let section_items: Vec<Item> = self.items[sec.header + 1..sec.end].to_vec();
        let indent = section_indent(&section_items);

        let mut last_occurrence: HashMap<String, usize> = HashMap::new();
        for (offset, item) in section_items.iter().enumerate() {
            if let Item::Param { key, .. } = item {
                last_occurrence.insert(key.to_lowercase(), offset);
            }
        }

        // 值本来就相同的 Set 视为无操作，连重复行也不动。
        let mut applied = vec![false; edits.len()];
        let mut noop: HashSet<String> = HashSet::new();
        for (index, edit) in edits.iter().enumerate() {
            let Some(value) = &edit.value else { continue };
            let Some(&offset) = last_occurrence.get(&edit.key.to_lowercase()) else {
                continue;
            };
            if let Item::Param { value: current, .. } = &section_items[offset] {
                if current == value {
                    noop.insert(edit.key.to_lowercase());
                    applied[index] = true;
                }
            }
        }

        let mut rebuilt: Vec<Item> = Vec::with_capacity(section_items.len() + edits.len());
        for (offset, item) in section_items.iter().enumerate() {
            match item {
                Item::Param { key, value, raw } => {
                    let lower = key.to_lowercase();
                    if noop.contains(&lower) {
                        rebuilt.push(item.clone());
                        continue;
                    }
                    let Some(index) = edits.iter().position(|e| e.key.eq_ignore_ascii_case(key))
                    else {
                        rebuilt.push(item.clone());
                        continue;
                    };
                    applied[index] = true;
                    match &edits[index].value {
                        None => { /* 删除 */ }
                        Some(new_value) => {
                            if last_occurrence.get(&lower) != Some(&offset) {
                                // 重复定义只保留最后一条（Samba 以最后一条为准）。
                                continue;
                            }
                            if new_value == value {
                                rebuilt.push(item.clone());
                            } else {
                                rebuilt.push(Item::Param {
                                    key: key.clone(),
                                    value: new_value.clone(),
                                    raw: format!(
                                        "{}{} = {}",
                                        leading_ws(raw),
                                        key,
                                        render_value(new_value)
                                    ),
                                });
                            }
                        }
                    }
                }
                other => rebuilt.push(other.clone()),
            }
        }

        for (index, edit) in edits.iter().enumerate() {
            if applied[index] {
                continue;
            }
            if let Some(value) = &edit.value {
                rebuilt.push(Item::Param {
                    key: edit.key.clone(),
                    value: value.clone(),
                    raw: format!("{indent}{} = {}", edit.key, render_value(value)),
                });
            }
        }

        self.items.splice(sec.header + 1..sec.end, rebuilt);
        Ok(())
    }
}

fn classify(line: &str) -> Item {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Item::Blank(line.to_string());
    }
    if trimmed.starts_with('#') || trimmed.starts_with(';') {
        return Item::Comment(line.to_string());
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() >= 2 {
        let name = trimmed[1..trimmed.len() - 1].trim();
        if !name.is_empty() {
            return Item::Section {
                name: name.to_string(),
                raw: line.to_string(),
            };
        }
    }
    if let Some(eq) = line.find('=') {
        let key = line[..eq].trim();
        if !key.is_empty() {
            return Item::Param {
                key: key.to_string(),
                value: parse_value(line[eq + 1..].trim()),
                raw: line.to_string(),
            };
        }
    }
    Item::Other(line.to_string())
}

fn parse_value(raw: &str) -> String {
    if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
        let inner = &raw[1..raw.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        raw.to_string()
    }
}

fn render_value(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let needs_quotes = value.chars().next().map_or(false, char::is_whitespace)
        || value.chars().last().map_or(false, char::is_whitespace)
        || value.chars().any(|c| matches!(c, '"' | '\\' | '#' | ';'));
    if !needs_quotes {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn leading_ws(s: &str) -> &str {
    let end = s.find(|c: char| c != ' ' && c != '\t').unwrap_or(s.len());
    &s[..end]
}

fn section_indent(items: &[Item]) -> String {
    for item in items {
        if let Item::Param { raw, .. } = item {
            let ws = leading_ws(raw);
            if !ws.is_empty() {
                return ws.to_string();
            }
        }
    }
    DEFAULT_INDENT.to_string()
}

fn normalize_edits(edits: &[Edit]) -> Result<Vec<NormEdit>, String> {
    let mut out: Vec<NormEdit> = Vec::new();
    for edit in edits {
        let (key, value) = match edit {
            Edit::Set { key, value } => (key.as_str(), Some(value)),
            Edit::Delete { key } => (key.as_str(), None),
        };
        let key = key.trim();
        validate_param_key(key)?;
        if let Some(value) = value {
            validate_param_value(value)?;
        }
        if let Some(existing) = out
            .iter_mut()
            .find(|entry| entry.key.eq_ignore_ascii_case(key))
        {
            existing.value = value.cloned();
        } else {
            out.push(NormEdit {
                key: key.to_string(),
                value: value.cloned(),
            });
        }
    }
    Ok(out)
}

pub fn validate_section_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("配置段名称不能为空".to_string());
    }
    if name.chars().count() > 128 {
        return Err("配置段名称过长（最多 128 个字符）".to_string());
    }
    for c in name.chars() {
        if matches!(c, '[' | ']' | '/' | '\\' | '\n' | '\r' | '\0') {
            return Err(format!("配置段名称不能包含字符 {c:?}"));
        }
    }
    Ok(())
}

pub fn validate_param_key(key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("参数名不能为空".to_string());
    }
    if key.chars().count() > 128 {
        return Err("参数名过长（最多 128 个字符）".to_string());
    }
    for c in key.chars() {
        if matches!(c, '=' | '[' | ']' | '#' | ';' | '\n' | '\r' | '\0') {
            return Err(format!("参数名不能包含字符 {c:?}"));
        }
    }
    Ok(())
}

pub fn validate_param_value(value: &str) -> Result<(), String> {
    if value.chars().count() > 8_192 {
        return Err("参数值过长（最多 8192 个字符）".to_string());
    }
    for c in value.chars() {
        if matches!(c, '\n' | '\r' | '\0') {
            return Err("参数值不能包含换行符或 NUL".to_string());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        include_str!("../examples/smb.conf").to_string()
    }

    #[test]
    fn roundtrip_examples() {
        let samples = [
            String::new(),
            "\n".to_string(),
            "\n\n".to_string(),
            "\r\n".to_string(),
            "no trailing newline".to_string(),
            "\u{feff}[global]\n   workgroup = X\n".to_string(),
            sample(),
            "; only a comment\n".to_string(),
            "[global]\n\tweird line without equals\n\tkey = value\n".to_string(),
            "[global]\r\n   workgroup = WORKGROUP\r\n\r\n[share]\r\n   path = /srv\r\n".to_string(),
        ];
        for text in samples {
            assert_eq!(
                Doc::parse(&text).render(),
                text,
                "roundtrip failed for {text:?}"
            );
        }
    }

    #[test]
    fn parses_sections_and_values() {
        let doc = Doc::parse(&sample());
        assert_eq!(doc.global().unwrap().name, "global");
        assert_eq!(doc.shares().len(), 2);
        assert_eq!(doc.get("global", "workgroup").as_deref(), Some("WORKGROUP"));
        assert_eq!(
            doc.get("public", "path").as_deref(),
            Some("/srv/samba/public")
        );
        assert_eq!(doc.get("homes", "valid users").as_deref(), Some("%S"));
        assert!(doc.get("public", "missing").is_none());
    }

    #[test]
    fn quoted_values_are_unquoted_and_escaped() {
        let doc = Doc::parse("[s]\n\tcomment = \"a \\\"b\\\" c\"\n\tempty =\n");
        assert_eq!(doc.get("s", "comment").as_deref(), Some("a \"b\" c"));
        assert_eq!(doc.get("s", "empty").as_deref(), Some(""));

        let mut doc = doc;
        doc.apply_edits(
            "s",
            &[Edit::Set {
                key: "comment".to_string(),
                value: "Hello # world; \"x\"".to_string(),
            }],
        )
        .unwrap();
        let text = doc.render();
        assert!(
            text.contains(r#"comment = "Hello # world; \"x\"""#),
            "{text}"
        );
    }

    #[test]
    fn edit_keeps_comments_and_order() {
        let mut doc = Doc::parse(&sample());
        doc.apply_edits(
            "global",
            &[Edit::Set {
                key: "workgroup".to_string(),
                value: "HOME".to_string(),
            }],
        )
        .unwrap();
        let text = doc.render();
        assert!(text.contains("# Samba 配置示例"));
        assert!(text.contains("   workgroup = HOME"));
        assert!(text.contains("   server string = Samba Server %v"));
        let workgroup = text.find("workgroup = HOME").unwrap();
        let server_string = text.find("server string").unwrap();
        assert!(workgroup < server_string);
    }

    #[test]
    fn unchanged_value_keeps_raw_line() {
        let mut doc = Doc::parse("[global]\n  workgroup    =   MYGROUP\n");
        doc.apply_edits(
            "global",
            &[Edit::Set {
                key: "workgroup".to_string(),
                value: "MYGROUP".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(doc.render(), "[global]\n  workgroup    =   MYGROUP\n");
    }

    #[test]
    fn delete_and_add_params() {
        let mut doc = Doc::parse(&sample());
        doc.apply_edits(
            "global",
            &[
                Edit::Delete {
                    key: "map to guest".to_string(),
                },
                Edit::Set {
                    key: "wins support".to_string(),
                    value: "yes".to_string(),
                },
            ],
        )
        .unwrap();
        let text = doc.render();
        assert!(!text.contains("map to guest"));
        assert!(text.contains("   wins support = yes"));
        assert!(text.find("wins support").unwrap() < text.find("[public]").unwrap());
    }

    #[test]
    fn duplicate_params_collapse_on_change() {
        let mut doc = Doc::parse("[global]\n\tcomment = a\n\tcomment = b\n");
        doc.apply_edits(
            "global",
            &[Edit::Set {
                key: "comment".to_string(),
                value: "c".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(doc.render(), "[global]\n\tcomment = c\n");

        let mut doc = Doc::parse("[global]\n\tcomment = a\n\tcomment = b\n");
        doc.apply_edits(
            "global",
            &[Edit::Set {
                key: "comment".to_string(),
                value: "b".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(doc.render(), "[global]\n\tcomment = a\n\tcomment = b\n");
    }

    #[test]
    fn add_and_delete_sections() {
        let mut doc = Doc::parse("[global]\n   workgroup = X\n");
        doc.add_section("docs", &[("path".to_string(), "/srv/docs".to_string())])
            .unwrap();
        assert_eq!(
            doc.render(),
            "[global]\n   workgroup = X\n\n[docs]\n   path = /srv/docs\n"
        );
        assert!(doc.add_section("docs", &[]).is_err());
        assert!(doc.add_section("bad/name", &[]).is_err());

        let mut doc = Doc::parse("[global]\n   x = 1\n\n[a]\n   path = /a\n\n[b]\n   path = /b\n");
        assert!(doc.delete_section("a"));
        assert_eq!(doc.render(), "[global]\n   x = 1\n\n[b]\n   path = /b\n");
        assert!(!doc.delete_section("missing"));
    }

    #[test]
    fn new_section_uses_file_indent_and_default() {
        let mut doc = Doc::parse("[global]\n\tworkgroup = X\n");
        doc.add_section("docs", &[("path".to_string(), "/srv/docs".to_string())])
            .unwrap();
        assert!(doc.render().contains("\tpath = /srv/docs"));

        let mut doc = Doc::parse("");
        doc.add_section("docs", &[("path".to_string(), "/x".to_string())])
            .unwrap();
        assert_eq!(doc.render(), "[docs]\n    path = /x\n");
    }

    #[test]
    fn ensure_global_creates_section() {
        let mut doc = Doc::parse("[public]\n   path = /srv\n");
        doc.ensure_global();
        assert_eq!(doc.global_count(), 1);
        assert!(doc.render().starts_with("[global]\n"));
        doc.ensure_global();
        assert_eq!(doc.global_count(), 1);
    }

    #[test]
    fn validation_rules() {
        assert!(validate_section_name("ok").is_ok());
        assert!(validate_section_name("bad[name]").is_err());
        assert!(validate_section_name("a/b").is_err());
        assert!(validate_param_key("path").is_ok());
        assert!(validate_param_key("bad=key").is_err());
        assert!(validate_param_key("bad#key").is_err());
        assert!(validate_param_value("a = b").is_ok());
        assert!(validate_param_value("a\nb").is_err());
    }

    #[test]
    fn crlf_and_bom_are_preserved_on_edit() {
        let mut doc = Doc::parse("\u{feff}[global]\r\n   workgroup = A\r\n");
        doc.apply_edits(
            "global",
            &[Edit::Set {
                key: "workgroup".to_string(),
                value: "B".to_string(),
            }],
        )
        .unwrap();
        assert_eq!(doc.render(), "\u{feff}[global]\r\n   workgroup = B\r\n");
    }

    #[test]
    fn params_and_preamble_detection() {
        let doc = Doc::parse("preamble = 1\n[global]\n   a = 1\n   a = 2\n[share]\n   path = /x\n");
        assert!(doc.has_preamble_params());
        assert_eq!(doc.param_count(), 4);
        let global = doc.global().unwrap();
        assert_eq!(doc.section_param_count(&global), 2);
        let params = doc.params(&global);
        assert_eq!(params.len(), 2);
        assert_eq!(params[1].value, "2");
    }
}
