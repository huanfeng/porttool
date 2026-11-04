//! 串口转发（复用 comhub-core 逻辑）

use super::{Forwarder, ForwardStatus, RuleType, State};
use crate::config::SerialRule;
use async_trait::async_trait;

pub struct SerialForwarder {
    rule: SerialRule,
    // TODO: 后续从 comhub-core 迁移
}

impl SerialForwarder {
    pub fn new(rule: SerialRule) -> Self {
        Self { rule }
    }
}

#[async_trait]
impl Forwarder for SerialForwarder {
    async fn start(&self) -> anyhow::Result<()> {
        tracing::info!("启动串口转发: {}", self.rule.name);
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        tracing::info!("停止串口转发: {}", self.rule.name);
        Ok(())
    }

    fn status(&self) -> ForwardStatus {
        ForwardStatus {
            name: self.rule.name.clone(),
            rule_type: RuleType::Serial,
            state: State::Stopped,
            detail: "未实现".into(),
            clients: 0,
        }
    }

    fn name(&self) -> &str {
        &self.rule.name
    }
}
