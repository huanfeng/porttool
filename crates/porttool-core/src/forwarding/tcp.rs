//! TCP 端口转发

use super::{Forwarder, ForwardStatus, RuleType, State};
use crate::config::TcpRule;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::io;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;

pub struct TcpForwarder {
    rule: TcpRule,
    state: Arc<RwLock<ForwarderState>>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

struct ForwarderState {
    state: State,
    detail: String,
    active_connections: usize,
}

impl TcpForwarder {
    pub fn new(rule: TcpRule) -> Self {
        Self {
            rule,
            state: Arc::new(RwLock::new(ForwarderState {
                state: State::Stopped,
                detail: "未启动".into(),
                active_connections: 0,
            })),
            task: Arc::new(Mutex::new(None)),
        }
    }

    async fn update_state(&self, state: State, detail: String) {
        let mut s = self.state.write().await;
        s.state = state;
        s.detail = detail;
    }
}

#[async_trait]
impl Forwarder for TcpForwarder {
    async fn start(&self) -> anyhow::Result<()> {
        // 检查是否已经运行
        {
            let task = self.task.lock().await;
            if task.is_some() {
                return Ok(());
            }
        }

        tracing::info!("启动 TCP 转发: {} ({} -> {})",
            self.rule.name, self.rule.listen, self.rule.target);

        // 解析地址
        let listen_addr = self.rule.listen.parse::<std::net::SocketAddr>()
            .map_err(|e| anyhow::anyhow!("监听地址格式错误: {}", e))?;
        let target_addr = self.rule.target.parse::<std::net::SocketAddr>()
            .map_err(|e| anyhow::anyhow!("目标地址格式错误: {}", e))?;

        // 绑定监听
        self.update_state(State::Starting, "正在绑定端口...".into()).await;
        let listener = TcpListener::bind(listen_addr).await
            .map_err(|e| {
                let msg = format!("绑定端口失败: {}", e);
                let state = self.state.clone();
                tokio::spawn(async move {
                    let mut s = state.write().await;
                    s.state = State::Failed;
                    s.detail = msg;
                });
                anyhow::anyhow!("绑定 {} 失败: {}", listen_addr, e)
            })?;

        self.update_state(
            State::Running,
            format!("监听中 ({} -> {})", self.rule.listen, self.rule.target)
        ).await;

        // 启动转发任务
        let state = self.state.clone();
        let name = self.rule.name.clone();
        let max_conns = self.rule.max_connections;
        let handle = tokio::spawn(async move {
            tracing::debug!("TCP 转发任务启动: {}", name);

            loop {
                match listener.accept().await {
                    Ok((client, client_addr)) => {
                        // 检查连接数限制（先读后判断，避免竞态条件）
                        let current = {
                            let s = state.read().await;
                            s.active_connections
                        };

                        if current >= max_conns {
                            tracing::warn!(
                                "[{}] 达到最大连接数 {}, 拒绝新连接: {}",
                                name, max_conns, client_addr
                            );
                            drop(client); // 立即关闭连接
                            continue;
                        }

                        tracing::debug!("[{}] 新连接: {} ({}/{})", name, client_addr, current + 1, max_conns);

                        // 先增加计数（在 spawn 之前，防止竞态）
                        {
                            let mut s = state.write().await;
                            s.active_connections += 1;
                        }

                        // 为每个连接启动独立的转发任务
                        let target = target_addr;
                        let state_clone = state.clone();
                        let name_clone = name.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(client, target, &name_clone).await {
                                tracing::debug!("[{}] 连接处理失败: {}", name_clone, e);
                            }

                            // 减少活动连接计数
                            let mut s = state_clone.write().await;
                            s.active_connections = s.active_connections.saturating_sub(1);
                        });
                    }
                    Err(e) => {
                        tracing::error!("[{}] accept 失败: {}", name, e);
                        // 短暂等待后继续
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                }
            }
        });

        *self.task.lock().await = Some(handle);
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        tracing::info!("停止 TCP 转发: {}", self.rule.name);

        let mut task = self.task.lock().await;
        if let Some(handle) = task.take() {
            handle.abort();
            self.update_state(State::Stopped, "已停止".into()).await;
        }
        Ok(())
    }

    fn status(&self) -> ForwardStatus {
        // 使用 try_read 避免在 async 上下文中阻塞
        let state = self.state.try_read()
            .map(|s| (s.state, s.detail.clone(), s.active_connections))
            .unwrap_or((State::Stopped, "状态未知".into(), 0));

        ForwardStatus {
            name: self.rule.name.clone(),
            rule_type: RuleType::Tcp,
            state: state.0,
            detail: state.1,
            clients: state.2,
        }
    }

    fn name(&self) -> &str {
        &self.rule.name
    }
}

/// 处理单个 TCP 连接的双向转发
async fn handle_connection(
    mut client: TcpStream,
    target_addr: std::net::SocketAddr,
    name: &str,
) -> anyhow::Result<()> {
    // 连接到目标服务器
    let mut server = TcpStream::connect(target_addr).await
        .map_err(|e| anyhow::anyhow!("连接目标 {} 失败: {}", target_addr, e))?;

    tracing::debug!("[{}] 已连接到目标: {}", name, target_addr);

    // 双向数据转发
    match io::copy_bidirectional(&mut client, &mut server).await {
        Ok((client_to_server, server_to_client)) => {
            tracing::debug!(
                "[{}] 连接关闭: 上行 {} 字节, 下行 {} 字节",
                name, client_to_server, server_to_client
            );
        }
        Err(e) => {
            tracing::debug!("[{}] 数据转发错误: {}", name, e);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn start_echo_server() -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 1024];
                    while let Ok(n) = socket.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        let _ = socket.write_all(&buf[..n]).await;
                    }
                });
            }
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        addr
    }

    #[tokio::test]
    async fn tcp_forward_connects_and_proxies() {
        // 启动 echo 服务器
        let echo_addr = start_echo_server().await;

        // 创建转发规则
        let rule = TcpRule {
            name: "test".into(),
            listen: "127.0.0.1:0".into(), // 随机端口
            target: echo_addr.to_string(),
            max_connections: 1000,
            enabled: true,
        };

        // 先绑定监听端口以获取实际地址
        let listener = TcpListener::bind(&rule.listen).await.unwrap();
        let listen_addr = listener.local_addr().unwrap();
        drop(listener);

        // 更新规则使用实际地址
        let rule = TcpRule {
            listen: listen_addr.to_string(),
            max_connections: 1000,
            ..rule
        };

        let forwarder = TcpForwarder::new(rule);

        // 启动转发
        forwarder.start().await.unwrap();

        // 等待转发器就绪
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 验证状态
        let status = forwarder.status();
        assert_eq!(status.state, State::Running);

        // 连接到转发端口
        let mut client = TcpStream::connect(listen_addr).await.unwrap();

        // 发送数据
        let test_data = b"Hello, TCP Forwarder!";
        client.write_all(test_data).await.unwrap();

        // 读取回显
        let mut buf = vec![0u8; 1024];
        let n = client.read(&mut buf).await.unwrap();

        assert_eq!(&buf[..n], test_data);

        // 停止转发
        forwarder.stop().await.unwrap();

        let status = forwarder.status();
        assert_eq!(status.state, State::Stopped);
    }

    #[tokio::test]
    async fn tcp_forward_handles_multiple_connections() {
        let echo_addr = start_echo_server().await;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = listener.local_addr().unwrap();
        drop(listener);

        let rule = TcpRule {
            name: "multi-test".into(),
            listen: listen_addr.to_string(),
            target: echo_addr.to_string(),
            max_connections: 1000,
            enabled: true,
        };

        let forwarder = TcpForwarder::new(rule);
        forwarder.start().await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 创建多个并发连接
        let mut handles = vec![];
        for i in 0..5 {
            let addr = listen_addr;
            let handle = tokio::spawn(async move {
                let mut client = TcpStream::connect(addr).await.unwrap();
                let msg = format!("Message {}", i);
                client.write_all(msg.as_bytes()).await.unwrap();

                let mut buf = vec![0u8; 1024];
                let n = client.read(&mut buf).await.unwrap();
                assert_eq!(&buf[..n], msg.as_bytes());
            });
            handles.push(handle);
        }

        // 等待所有连接完成
        for handle in handles {
            handle.await.unwrap();
        }

        forwarder.stop().await.unwrap();
    }

    #[tokio::test]
    async fn tcp_forward_fails_on_invalid_listen_address() {
        let rule = TcpRule {
            name: "bad-listen".into(),
            listen: "999.999.999.999:9999".into(),
            target: "127.0.0.1:80".into(),
            max_connections: 1000,
            enabled: true,
        };

        let forwarder = TcpForwarder::new(rule);
        let result = forwarder.start().await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn tcp_forward_handles_target_connection_failure() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = listener.local_addr().unwrap();
        drop(listener);

        let rule = TcpRule {
            name: "bad-target".into(),
            listen: listen_addr.to_string(),
            target: "127.0.0.1:1".into(), // 不存在的服务
            max_connections: 100,
            enabled: true,
        };

        let forwarder = TcpForwarder::new(rule);
        forwarder.start().await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 连接应该成功，但数据转发会失败
        let result = TcpStream::connect(listen_addr).await;
        assert!(result.is_ok());

        // 连接会很快关闭（因为目标不可达）
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        forwarder.stop().await.unwrap();
    }
}
