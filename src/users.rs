//! Samba 用户列表解析与 Linux 用户候选筛选。

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SambaUser {
    pub name: String,
    pub uid: Option<u32>,
    pub full_name: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    pub uid: u32,
    pub full_name: String,
}

/// 解析 `pdbedit -L`：`username:uid:full name`。
pub fn parse_list(output: &str) -> Vec<SambaUser> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, ':');
            let name = parts.next()?.trim();
            if name.is_empty() {
                return None;
            }
            let uid = parts.next().and_then(|value| value.trim().parse().ok());
            let full_name = parts.next().unwrap_or("").trim().to_string();
            Some(SambaUser {
                name: name.to_string(),
                uid,
                full_name,
                enabled: true,
            })
        })
        .collect()
}

/// 解析 `pdbedit -Lv`，提取启用状态和全名。
///
/// 不同 Samba 版本输出字段略有差异，这里同时接受 `Unix username:` 和
/// `username:`，并在遇到 `Account Flags:` 时读取 `D` 标志。
pub fn parse_verbose(output: &str) -> Vec<SambaUser> {
    let mut users = Vec::new();
    let mut current: Option<SambaUser> = None;
    let mut flags = String::new();
    let mut full_name = String::new();

    let finish = |current: &mut Option<SambaUser>,
                  flags: &str,
                  full_name: &str,
                  users: &mut Vec<SambaUser>| {
        if let Some(mut user) = current.take() {
            user.enabled = !flags.contains('D');
            if !full_name.trim().is_empty() {
                user.full_name = full_name.trim().to_string();
            }
            users.push(user);
        }
    };

    for line in output.lines() {
        let trimmed = line.trim();
        let name = if let Some(value) = trimmed.strip_prefix("Unix username:") {
            Some(value.trim())
        } else {
            trimmed.strip_prefix("username:").map(|value| value.trim())
        };

        if let Some(name) = name {
            if !name.is_empty() {
                finish(&mut current, &flags, &full_name, &mut users);
                flags.clear();
                full_name.clear();
                current = Some(SambaUser {
                    name: name.to_string(),
                    uid: None,
                    full_name: String::new(),
                    enabled: true,
                });
                continue;
            }
        }

        let Some(user) = current.as_mut() else {
            continue;
        };

        if let Some(value) = trimmed.strip_prefix("user ID/Group:") {
            user.uid = value
                .trim()
                .split('/')
                .next()
                .and_then(|uid| uid.trim().parse().ok());
        } else if let Some(value) = trimmed.strip_prefix("Full Name:") {
            full_name = value.trim().to_string();
        } else if let Some(value) = trimmed.strip_prefix("Account Flags:") {
            flags = value
                .trim()
                .trim_matches(|c| c == '[' || c == ']')
                .to_string();
        }
    }

    finish(&mut current, &flags, &full_name, &mut users);
    users
}

pub fn merge(list: &str, verbose: &str) -> Vec<SambaUser> {
    let mut users = parse_list(list);
    let verbose_users = parse_verbose(verbose);

    for user in &mut users {
        if let Some(extra) = verbose_users.iter().find(|extra| extra.name == user.name) {
            user.enabled = extra.enabled;
            if user.uid.is_none() {
                user.uid = extra.uid;
            }
            if user.full_name.is_empty() {
                user.full_name = extra.full_name.clone();
            }
        }
    }

    users
}

/// 从 `/etc/passwd` 中筛选可创建 Samba 账户的普通用户。
pub fn linux_candidates(passwd: &str, existing: &[SambaUser]) -> Vec<Candidate> {
    let mut candidates = Vec::new();

    for line in passwd.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 7 {
            continue;
        }
        let name = fields[0];
        let Ok(uid) = fields[2].parse::<u32>() else {
            continue;
        };
        if !(1000..65534).contains(&uid) || name == "nobody" {
            continue;
        }
        if existing
            .iter()
            .any(|user| user.name.eq_ignore_ascii_case(name))
        {
            continue;
        }
        let full_name = fields[4].split(',').next().unwrap_or("").trim().to_string();
        candidates.push(Candidate {
            name: name.to_string(),
            uid,
            full_name,
        });
    }

    candidates.sort_by(|left, right| left.name.cmp(&right.name));
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_list() {
        let users = parse_list("admin:1000:Administrator\nbob:1001:Bob\n");
        assert_eq!(users.len(), 2);
        assert_eq!(users[0].name, "admin");
        assert_eq!(users[0].uid, Some(1000));
        assert_eq!(users[1].full_name, "Bob");
    }

    #[test]
    fn parses_verbose_flags() {
        let output = "\
---------------
Unix username:        admin
user ID/Group:        1000/1000
Full Name:            Administrator
Account Flags:        [U          ]
---------------
Unix username:        bob
user ID/Group:        1001/1001
Full Name:            Bob
Account Flags:        [DU         ]
";
        let users = parse_verbose(output);
        assert_eq!(users.len(), 2);
        assert!(users[0].enabled);
        assert!(!users[1].enabled);
        assert_eq!(users[0].full_name, "Administrator");
    }

    #[test]
    fn merges_list_and_verbose() {
        let users = merge(
            "admin:1000:\nbob:1001:Bob\n",
            "Unix username: admin\nAccount Flags: [U]\n\
             Unix username: bob\nAccount Flags: [DU]\n",
        );
        assert_eq!(users.len(), 2);
        assert!(users[0].enabled);
        assert!(!users[1].enabled);
        assert_eq!(users[1].full_name, "Bob");
    }

    #[test]
    fn filters_linux_candidates() {
        let passwd = "\
root:x:0:0:root:/root:/bin/bash
systemd:x:999:999:system:/:/sbin/nologin
admin:x:1000:1000:Admin:/home/admin:/bin/bash
bob:x:1001:1001:Bob:/home/bob:/bin/bash
nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin
";
        let existing = vec![SambaUser {
            name: "admin".to_string(),
            uid: Some(1000),
            full_name: String::new(),
            enabled: true,
        }];
        let candidates = linux_candidates(passwd, &existing);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "bob");
        assert_eq!(candidates[0].uid, 1001);
    }
}
