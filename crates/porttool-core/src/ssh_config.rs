//! 从用户的 `~/.ssh/config` 读取可选的 SSH 服务器别名，供界面和 HTTP 提供保定选择。
//!
//! 只读取显式写出的 `Host 别名` 条目：通配符和取反模式是匹配模式，不是可选的地址；
//! `Include` 引用的文件不递归展开。

use std::path::PathBuf;

/// 解析 SSH 配置文本中的显式主机别名，按出现顺序去重。
pub fn aliases(text: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some(end) = line.find(|c: char| c.is_ascii_whitespace() || c == '=') else {
            continue;
        };
        if !line[..end].eq_ignore_ascii_case("host") {
            continue;
        }
        for alias in line[end..]
            .trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '=')
            .split_whitespace()
        {
            if !alias.contains(['*', '?', '!'])
                && valid_target(alias).is_ok()
                && !aliases.iter().any(|a| a == alias)
            {
                aliases.push(alias.to_string());
            }
        }
    }
    aliases
}

/// SSH 目标（user@host 或主机别名）能否原样传给 `ssh` 命令行。
pub fn valid_target(target: &str) -> Result<(), &'static str> {
    if target.is_empty() || target.starts_with('-') || target.chars().any(char::is_whitespace) {
        Err("请输入有效的 SSH 目标")
    } else {
        Ok(())
    }
}

/// 用户 SSH 配置的位置：`%USERPROFILE%\.ssh\config`（无 USERPROFILE 时用 `$HOME/.ssh/config`）。
pub fn config_path() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join(".ssh").join("config"))
}

/// 读取配置文件里的别名列表；文件不存在或读不了时返回空。
pub fn available_servers() -> Vec<String> {
    let Some(path) = config_path() else {
        return Vec::new();
    };
    std::fs::read_to_string(path)
        .map(|text| aliases(&text))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_explicit_aliases_only() {
        let config = r#"
# comment
Host *
    HostName 127.0.0.1

Host dev-board !staging-*
    User root
    HostName 192.168.5.88

Host=jump.example.com op-jump
    IdentityFile ~/.ssh/id_ed25519

Include other.conf
"#;
        assert_eq!(aliases(config), vec!["dev-board", "jump.example.com", "op-jump"]);
        assert!(aliases("").is_empty());
    }

    #[test]
    fn rejects_targets_that_could_be_options() {
        assert!(valid_target("user@host").is_ok());
        assert!(valid_target("dev-board").is_ok());
        assert!(valid_target("-oProxyCommand=calc").is_err());
        assert!(valid_target("two words").is_err());
        assert!(valid_target("").is_err());
    }
}
