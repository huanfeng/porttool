//! SSH 端口转发（复用 porttool 逻辑）

use super::{Forwarder, ForwardStatus, RuleType, State};
use crate::config::SshRule;
use async_trait::async_trait;

pub struct SshForwarder {
    rule: SshRule,
    // TODO: 后续从 porttool 迁移 process.rs
}

impl SshForwarder {
    pub fn new(rule: SshRule) -> Self {
        Self { rule }
    }
}

#[async_trait]
impl Forwarder for SshForwarder {
    async fn start(&self) -> anyhow::Result<()> {
        // TODO: 启动 ssh 进程
        tracing::info!("启动 SSH 转发: {}", self.rule.name);
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        // TODO: 停止 ssh 进程
        tracing::info!("停止 SSH 转发: {}", self.rule.name);
        Ok(())
    }

    fn status(&self) -> ForwardStatus {
        ForwardStatus {
            name: self.rule.name.clone(),
            rule_type: RuleType::Ssh,
            state: State::Stopped,
            detail: "未实现".into(),
            clients: 0,
        }
    }

    fn name(&self) -> &str {
        &self.rule.name
    }
}
