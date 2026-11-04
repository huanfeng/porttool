//! 服务总入口：管理所有转发规则

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;

use crate::config::{Config, ForwardRule};
use crate::forwarding::{Forwarder, ForwardStatus, serial::SerialForwarder, ssh::SshForwarder, tcp::TcpForwarder};

/// 服务事件通知
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceEvent {
    /// 规则状态变化
    RuleChanged(String),
    /// 规则列表变化（重新加载配置后）
    RulesReloaded,
}

pub struct Service {
    config: RwLock<Config>,
    config_path: PathBuf,
    forwarders: RwLock<HashMap<String, Box<dyn Forwarder>>>,
}

impl Service {
    /// 启动服务
    pub async fn start(config: Config, config_path: PathBuf) -> Result<Arc<Self>> {
        config.validate()?;

        let service = Arc::new(Self {
            config: RwLock::new(config.clone()),
            config_path,
            forwarders: RwLock::new(HashMap::new()),
        });

        service.apply_config(config).await?;
        Ok(service)
    }

    /// 应用新配置
    async fn apply_config(&self, config: Config) -> Result<()> {
        let mut forwarders = self.forwarders.write().await;

        // 停止不在新配置中的规则
        forwarders.retain(|name, _| {
            config.rules.iter().any(|r| r.name() == name)
        });

        // 创建新规则的转发器
        for rule in &config.rules {
            if !rule.enabled() {
                continue;
            }

            let name = rule.name().to_string();
            if forwarders.contains_key(&name) {
                continue; // 已存在，跳过
            }

            let forwarder: Box<dyn Forwarder> = match rule {
                ForwardRule::Serial(r) => Box::new(SerialForwarder::new(r.clone())),
                ForwardRule::Ssh(r) => Box::new(SshForwarder::new(r.clone())),
                ForwardRule::Tcp(r) => Box::new(TcpForwarder::new(r.clone())),
            };

            forwarders.insert(name, forwarder);
        }

        Ok(())
    }

    /// 重新加载配置
    pub async fn reload(&self, config: Config) -> Result<()> {
        config.validate()?;
        *self.config.write().await = config.clone();
        self.apply_config(config).await
    }

    /// 列出所有规则状态
    pub async fn list_rules(&self) -> Vec<ForwardStatus> {
        self.forwarders
            .read()
            .await
            .values()
            .map(|f| f.status())
            .collect()
    }

    /// 启用规则
    pub async fn enable_rule(&self, name: &str) -> Result<()> {
        let forwarders = self.forwarders.read().await;
        let fwd = forwarders
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("规则不存在: {}", name))?;
        fwd.start().await
    }

    /// 停用规则
    pub async fn disable_rule(&self, name: &str) -> Result<()> {
        let forwarders = self.forwarders.read().await;
        let fwd = forwarders
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("规则不存在: {}", name))?;
        fwd.stop().await
    }

    /// 获取配置
    pub async fn config(&self) -> Config {
        self.config.read().await.clone()
    }

    /// 获取配置文件路径
    pub fn config_path(&self) -> &PathBuf {
        &self.config_path
    }

    /// 关闭服务
    pub async fn shutdown(&self) {
        let forwarders = self.forwarders.write().await;
        for (name, fwd) in forwarders.iter() {
            if let Err(e) = fwd.stop().await {
                tracing::warn!("停止规则 {} 失败: {}", name, e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TcpRule;

    #[tokio::test]
    async fn creates_service_with_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let mut config = Config::default();
        config.rules.push(ForwardRule::Tcp(TcpRule {
            name: "test".into(),
            listen: "127.0.0.1:8080".into(),
            target: "127.0.0.1:9090".into(),
            enabled: true,
        }));

        let service = Service::start(config, path).await.unwrap();
        let rules = service.list_rules().await;
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "test");
    }

    #[tokio::test]
    async fn reload_adds_and_removes_rules() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let mut config = Config::default();
        config.rules.push(ForwardRule::Tcp(TcpRule {
            name: "a".into(),
            listen: "127.0.0.1:8080".into(),
            target: "127.0.0.1:9090".into(),
            enabled: true,
        }));

        let service = Service::start(config.clone(), path).await.unwrap();
        assert_eq!(service.list_rules().await.len(), 1);

        // 添加新规则
        config.rules.push(ForwardRule::Tcp(TcpRule {
            name: "b".into(),
            listen: "127.0.0.1:8081".into(),
            target: "127.0.0.1:9091".into(),
            enabled: true,
        }));
        service.reload(config.clone()).await.unwrap();
        assert_eq!(service.list_rules().await.len(), 2);

        // 移除旧规则
        config.rules.retain(|r| r.name() != "a");
        service.reload(config).await.unwrap();
        let rules = service.list_rules().await;
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "b");
    }
}
