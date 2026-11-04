//! 端口转发实现

pub mod ssh;
pub mod tcp;
pub mod serial;

use async_trait::async_trait;

/// 转发器统一接口
#[async_trait]
pub trait Forwarder: Send + Sync {
    async fn start(&self) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
    fn status(&self) -> ForwardStatus;
    fn name(&self) -> &str;
}

/// 转发状态
#[derive(Debug, Clone, serde::Serialize)]
pub struct ForwardStatus {
    pub name: String,
    pub rule_type: RuleType,
    pub state: State,
    pub detail: String,
    /// 客户端数量（仅串口有意义）
    #[serde(default)]
    pub clients: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Stopped,
    Starting,
    Running,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleType {
    Serial,
    Ssh,
    Tcp,
}
