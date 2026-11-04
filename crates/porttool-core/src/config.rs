//! 配置文件（TOML）的读写与校验

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// 完整配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(rename = "rule")]
    pub rules: Vec<ForwardRule>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            rules: Vec::new(),
        }
    }
}

/// 服务器配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// HTTP 与 Terminal 服务共用的监听地址
    pub bind: String,
    pub http_port: u16,
    /// 日志根目录，为空时使用配置文件所在目录下的 `logs/`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_dir: Option<PathBuf>,
    /// HTTP API 认证 token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// 串口在内存中保留的行数
    pub buffer_lines: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: "0.0.0.0".into(),
            http_port: 7000,
            log_dir: None,
            token: None,
            buffer_lines: 20_000,
        }
    }
}

/// 转发规则（根据 type 字段区分类型）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ForwardRule {
    Serial(SerialRule),
    Ssh(SshRule),
    Tcp(TcpRule),
}

impl ForwardRule {
    pub fn name(&self) -> &str {
        match self {
            ForwardRule::Serial(r) => &r.name,
            ForwardRule::Ssh(r) => &r.name,
            ForwardRule::Tcp(r) => &r.name,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            ForwardRule::Serial(r) => r.enabled,
            ForwardRule::Ssh(r) => r.enabled,
            ForwardRule::Tcp(r) => r.enabled,
        }
    }
}

/// 串口转发规则
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SerialRule {
    pub name: String,
    /// USB 序列号（优先）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial_number: Option<String>,
    /// 串口名称（如 COM3）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub com: Option<String>,
    #[serde(default = "default_baud")]
    pub baud: u32,
    pub tcp_port: u16,
    #[serde(default)]
    pub mode: TerminalMode,
    #[serde(default)]
    pub encoding: Encoding,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// SSH 端口转发规则
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SshRule {
    pub name: String,
    /// SSH 目标（user@host 或主机别名）
    pub ssh_target: String,
    /// SSH 端口（可选，留空使用 SSH 配置）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_port: Option<u16>,
    /// 私钥路径（可选，留空使用系统配置和代理）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    #[serde(default)]
    pub direction: Direction,
    /// 监听端口
    pub local_port: u16,
    /// 目标地址
    #[serde(default = "default_localhost")]
    pub remote_host: String,
    /// 目标端口
    pub remote_port: u16,
    #[serde(default)]
    pub auto_reconnect: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// TCP 端口转发规则
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TcpRule {
    pub name: String,
    /// 监听地址（如 "127.0.0.1:8080"）
    pub listen: String,
    /// 目标地址（如 "192.168.1.100:80"）
    pub target: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// 本机 → 远端（-L）
    #[default]
    Local,
    /// 远端 → 本机（-R）
    Remote,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalMode {
    #[default]
    Telnet,
    Raw,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Encoding {
    #[default]
    #[serde(rename = "utf-8", alias = "utf8")]
    Utf8,
    #[serde(rename = "gbk")]
    Gbk,
}

fn default_baud() -> u32 {
    115_200
}

fn default_true() -> bool {
    true
}

fn default_localhost() -> String {
    "127.0.0.1".into()
}

impl Config {
    /// 跨平台默认配置路径
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("porttool")
            .join("config.toml")
    }

    /// 读取配置；文件不存在时返回默认配置
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("读取配置文件失败: {}", path.display()))?;
        let config: Self = toml::from_str(&text)
            .with_context(|| format!("解析配置文件失败: {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, toml::to_string_pretty(self)?)
            .with_context(|| format!("写入配置文件失败: {}", path.display()))
    }

    pub fn validate(&self) -> Result<()> {
        let mut names = HashSet::new();
        let mut tcp_ports = HashSet::from([self.server.http_port]);

        for rule in &self.rules {
            let name = rule.name();

            // 检查名称合法性
            if !is_valid_name(name) {
                bail!("规则名称 `{}` 不合法：只能使用字母、数字、- 和 _", name);
            }

            // 检查名称唯一性（不区分大小写）
            if !names.insert(name.to_ascii_lowercase()) {
                bail!("规则名称 `{}` 重复", name);
            }

            // 检查各类型特定字段
            match rule {
                ForwardRule::Serial(r) => {
                    if r.serial_number.is_none() && r.com.is_none() {
                        bail!("串口规则 `{}` 需要配置 serial_number 或 com", r.name);
                    }
                    if !tcp_ports.insert(r.tcp_port) {
                        bail!("串口规则 `{}` 的 tcp_port {} 与其它端口冲突", r.name, r.tcp_port);
                    }
                }
                ForwardRule::Ssh(r) => {
                    if r.ssh_target.trim().is_empty() {
                        bail!("SSH 规则 `{}` 的 ssh_target 不能为空", r.name);
                    }
                    if r.local_port == 0 || r.remote_port == 0 {
                        bail!("SSH 规则 `{}` 的端口必须在 1-65535 范围内", r.name);
                    }
                }
                ForwardRule::Tcp(r) => {
                    // 验证地址格式
                    if r.listen.parse::<std::net::SocketAddr>().is_err() {
                        bail!("TCP 规则 `{}` 的 listen 地址格式不正确", r.name);
                    }
                    if r.target.parse::<std::net::SocketAddr>().is_err() {
                        bail!("TCP 规则 `{}` 的 target 地址格式不正确", r.name);
                    }
                }
            }
        }
        Ok(())
    }

    /// 日志根目录：优先使用配置项，否则是配置文件旁边的 `logs/`
    pub fn log_dir(&self, config_path: &Path) -> PathBuf {
        match &self.server.log_dir {
            Some(dir) => dir.clone(),
            None => config_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("logs"),
        }
    }
}

pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_config_with_defaults() {
        let config: Config = toml::from_str(
            r#"
            [[rule]]
            type = "serial"
            name = "esp32"
            serial_number = "A50285BI"
            tcp_port = 7001
            "#,
        )
        .unwrap();
        assert_eq!(config.server.http_port, 7000);
        let ForwardRule::Serial(port) = &config.rules[0] else { panic!() };
        assert_eq!(port.baud, 115_200);
        assert_eq!(port.mode, TerminalMode::Telnet);
        assert_eq!(port.encoding, Encoding::Utf8);
        assert!(port.enabled);
        config.validate().unwrap();
    }

    #[test]
    fn parses_all_rule_types() {
        let config: Config = toml::from_str(
            r#"
            [[rule]]
            type = "serial"
            name = "esp32"
            com = "COM3"
            tcp_port = 7001

            [[rule]]
            type = "ssh"
            name = "dev-proxy"
            ssh_target = "user@host"
            local_port = 8080
            remote_port = 80

            [[rule]]
            type = "tcp"
            name = "db-forward"
            listen = "127.0.0.1:3306"
            target = "192.168.1.100:3306"
            "#,
        )
        .unwrap();
        assert_eq!(config.rules.len(), 3);
        config.validate().unwrap();
    }

    #[test]
    fn rejects_duplicate_names() {
        let config: Config = toml::from_str(
            r#"
            [[rule]]
            type = "tcp"
            name = "test"
            listen = "127.0.0.1:8080"
            target = "127.0.0.1:9090"

            [[rule]]
            type = "tcp"
            name = "TEST"
            listen = "127.0.0.1:8081"
            target = "127.0.0.1:9091"
            "#,
        )
        .unwrap();
        assert!(config.validate().is_err(), "大小写重名应报错");
    }

    #[test]
    fn rejects_port_conflicts() {
        let config: Config = toml::from_str(
            r#"
            [server]
            http_port = 7000

            [[rule]]
            type = "serial"
            name = "a"
            com = "COM1"
            tcp_port = 7000
            "#,
        )
        .unwrap();
        assert!(config.validate().is_err(), "与 HTTP 端口冲突应报错");
    }

    #[test]
    fn rejects_invalid_addresses() {
        let config: Config = toml::from_str(
            r#"
            [[rule]]
            type = "tcp"
            name = "bad"
            listen = "not-an-address"
            target = "127.0.0.1:80"
            "#,
        )
        .unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let mut config = Config::default();
        config.rules.push(ForwardRule::Serial(SerialRule {
            name: "esp32".into(),
            serial_number: Some("SN1".into()),
            com: None,
            baud: 921_600,
            tcp_port: 7001,
            mode: TerminalMode::Raw,
            encoding: Encoding::Gbk,
            enabled: false,
        }));

        config.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded, config);
    }
}
