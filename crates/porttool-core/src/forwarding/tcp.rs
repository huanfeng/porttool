//! TCP 端口转发

use super::{Forwarder, ForwardStatus, RuleType, State};
use crate::config::TcpRule;
use async_trait::async_trait;

pub struct TcpForwarder {
    rule: TcpRule,
    // TODO: 后续实现
}

impl TcpForwarder {
    pub fn new(rule: TcpRule) -> Self {
        Self { rule }
    }
}

#[async_trait]
impl Forwarder for TcpForwarder {
    async fn start(&self) -> anyhow::Result<()> {
        tracing::info!("启动 TCP 转发: {}", self.rule.name);
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        tracing::info!("停止 TCP 转发: {}", self.rule.name);
        Ok(())
    }

    fn status(&self) -> ForwardStatus {
        ForwardStatus {
            name: self.rule.name.clone(),
            rule_type: RuleType::Tcp,
            state: State::Stopped,
            detail: "未实现".into(),
            clients: 0,
        }
    }

    fn name(&self) -> &str {
        &self.rule.name
    }
}
