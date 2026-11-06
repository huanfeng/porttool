//! SSH 端口转发：托管系统 `ssh` 进程（`-L` / `-R`），支持瞬时网络故障后的自动重连
//!
//! 沿用原 porttool 的做法：不引入 SSH 库，认证、known_hosts、ssh_config 全部交给系统 ssh。

use super::{ForwardStatus, Forwarder, RuleType, State};
use crate::config::{Direction, SshRule};
use async_trait::async_trait;
use std::io;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// 创建 ssh 子进程的函数；测试时替换成伪造进程
type Spawner = Arc<dyn Fn(&SshRule) -> io::Result<Child> + Send + Sync>;

struct Shared {
    state: State,
    detail: String,
    /// 当前是第几次重连；0 表示不在重连
    attempt: u32,
}

pub struct SshForwarder {
    rule: SshRule,
    shared: Arc<StdMutex<Shared>>,
    task: Mutex<Option<JoinHandle<()>>>,
    spawner: Spawner,
}

impl SshForwarder {
    pub fn new(rule: SshRule) -> Self {
        Self::with_spawner(rule, Arc::new(spawn_ssh))
    }

    fn with_spawner(rule: SshRule, spawner: Spawner) -> Self {
        Self {
            rule,
            shared: Arc::new(StdMutex::new(Shared {
                state: State::Stopped,
                detail: "未启动".into(),
                attempt: 0,
            })),
            task: Mutex::new(None),
            spawner,
        }
    }
}

fn set_state(shared: &StdMutex<Shared>, state: State, detail: impl Into<String>) {
    if let Ok(mut s) = shared.lock() {
        s.state = state;
        s.detail = detail.into();
        s.attempt = 0;
    }
}

/// 进入「重连」阶段：状态仍是 `Starting`，同时记下这是第几次重连。
fn set_reconnecting(shared: &StdMutex<Shared>, attempt: u32, detail: impl Into<String>) {
    if let Ok(mut s) = shared.lock() {
        s.state = State::Starting;
        s.detail = detail.into();
        s.attempt = attempt;
    }
}

/// 拼出 ssh 命令行参数（不含程序名）
pub(crate) fn ssh_args(rule: &SshRule) -> Vec<String> {
    let mut args: Vec<String> = [
        "-v",
        "-N",
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "BatchMode=yes",
        "-o",
        "ServerAliveInterval=30",
        "-o",
        "ServerAliveCountMax=3",
        "-o",
        "ConnectTimeout=15",
    ]
    .map(String::from)
    .into();
    if let Some(port) = rule.ssh_port {
        args.extend(["-p".into(), port.to_string()]);
    }
    if let Some(path) = &rule.identity_file {
        args.extend(["-i".into(), path.clone()]);
    }
    args.push(
        match rule.direction {
            Direction::Local => "-L",
            Direction::Remote => "-R",
        }
        .into(),
    );
    args.push(forward_spec(rule));
    args.push(rule.ssh_target.clone());
    args
}

/// `127.0.0.1:本地端口:目标:目标端口`；IPv6 目标自动加方括号
fn forward_spec(rule: &SshRule) -> String {
    let host = rule.remote_host.trim_matches(|c| c == '[' || c == ']');
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    format!("127.0.0.1:{}:{}:{}", rule.local_port, host, rule.remote_port)
}

fn spawn_ssh(rule: &SshRule) -> io::Result<Child> {
    let mut command = Command::new("ssh");
    command
        .args(ssh_args(rule))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        // 任务被 abort（停止规则）或服务退出时，连带结束 ssh 进程
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    command.spawn()
}

/// 只有明确属于瞬时网络故障的诊断才自动重连；认证、主机密钥、配置、本地端口占用
/// 都需要用户处理，重试没有意义。
pub(crate) fn is_transient(detail: &str) -> bool {
    let s = detail.to_ascii_lowercase();
    [
        "connection reset",
        "connection timed out",
        "connection closed",
        "connection refused",
        "broken pipe",
        "network is unreachable",
        "no route to host",
        "could not resolve hostname",
        "temporary failure in name resolution",
        "not responding",
    ]
    .iter()
    .any(|p| s.contains(p))
        && ![
            "permission denied",
            "host key verification failed",
            "address already in use",
            "could not request local forwarding",
            "administratively prohibited",
        ]
        .iter()
        .any(|p| s.contains(p))
}

/// ssh -v 输出中表示转发已建立的那一行
fn is_ready_line(direction: Direction, line: &str) -> bool {
    match direction {
        Direction::Local => line.contains("Local forwarding listening on"),
        Direction::Remote => line.contains("remote forward success for"),
    }
}

/// 保留最近几行有效诊断，用于失败原因
fn push_error(errors: &mut Vec<String>, line: String) {
    if !line.starts_with("debug") && !line.trim().is_empty() {
        errors.push(line);
        if errors.len() > 4 {
            errors.remove(0);
        }
    }
}

/// 监督任务：启动 ssh → 等待退出 → 视情况退避重连
async fn supervise(rule: SshRule, shared: Arc<StdMutex<Shared>>, spawner: Spawner, first: Child) {
    let mut child = first;
    let mut retries = 0u32;
    loop {
        let Some(stderr) = child.stderr.take() else {
            set_state(&shared, State::Failed, "无法读取 SSH 输出");
            return;
        };

        let mut errors = Vec::new();
        let mut connected_at = None;
        let mut reader = BufReader::new(stderr);
        let mut buf = Vec::new();
        // ssh 退出后 stderr 关闭，读到 EOF 即进程结束
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                    if connected_at.is_none() && is_ready_line(rule.direction, &line) {
                        connected_at = Some(Instant::now());
                        set_state(&shared, State::Running, "运行中");
                    }
                    push_error(&mut errors, line);
                }
            }
        }
        let exit = child.wait().await;

        let detail = if errors.is_empty() {
            match exit {
                Ok(s) => format!("SSH 已退出 ({s})"),
                Err(e) => format!("无法检测 SSH 进程：{e}"),
            }
        } else {
            errors.join("\n")
        };

        if !rule.auto_reconnect || !is_transient(&detail) {
            set_state(&shared, State::Failed, format!("失败：{detail}"));
            return;
        }

        // 稳定运行过一段时间后，退避从头开始
        if connected_at.is_some_and(|at| at.elapsed() >= Duration::from_secs(30)) {
            retries = 0;
        }
        retries = retries.saturating_add(1);
        let delay = (1u64 << retries.min(5)).min(30);
        set_reconnecting(&shared, retries, format!("连接中断，{delay} 秒后重连：{detail}"));
        tokio::time::sleep(Duration::from_secs(delay)).await;

        match spawner(&rule) {
            Ok(c) => {
                child = c;
                set_reconnecting(&shared, retries, "正在重连…");
            }
            Err(e) => {
                set_state(&shared, State::Failed, format!("重连失败：{e}"));
                return;
            }
        }
    }
}

#[async_trait]
impl Forwarder for SshForwarder {
    async fn start(&self) -> anyhow::Result<()> {
        let mut task = self.task.lock().await;
        if task.as_ref().is_some_and(|t| !t.is_finished()) {
            return Ok(());
        }

        self.rule.check().map_err(|e| anyhow::anyhow!("{e}"))?;
        tracing::info!(
            "启动 SSH 转发: {} ({} {})",
            self.rule.name,
            self.rule.ssh_target,
            forward_spec(&self.rule)
        );

        set_state(&self.shared, State::Starting, "正在连接…");
        let child = (self.spawner)(&self.rule).map_err(|e| {
            let msg = format!("无法启动 ssh（请确认已安装 OpenSSH 客户端）: {e}");
            set_state(&self.shared, State::Failed, msg.clone());
            anyhow::anyhow!(msg)
        })?;

        *task = Some(tokio::spawn(supervise(
            self.rule.clone(),
            self.shared.clone(),
            self.spawner.clone(),
            child,
        )));
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        tracing::info!("停止 SSH 转发: {}", self.rule.name);
        if let Some(handle) = self.task.lock().await.take() {
            // abort 会 drop 持有的 Child，kill_on_drop 负责结束 ssh 进程
            handle.abort();
            let _ = handle.await;
        }
        set_state(&self.shared, State::Stopped, "已停止");
        Ok(())
    }

    fn status(&self) -> ForwardStatus {
        let (state, detail, attempt) = self
            .shared
            .lock()
            .map(|s| (s.state, s.detail.clone(), s.attempt))
            .unwrap_or((State::Failed, "状态不可用".into(), 0));
        ForwardStatus {
            name: self.rule.name.clone(),
            rule_type: RuleType::Ssh,
            state,
            detail,
            clients: 0,
            endpoint: None,
            attempt,
        }
    }

    fn name(&self) -> &str {
        &self.rule.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 被测试进程以子进程方式重新调用，扮演 ssh：向 stderr 输出指定诊断
    #[test]
    #[ignore]
    fn fixture() {
        match std::env::var("PORTTOOL_TEST_SSH").as_deref() {
            Ok("running") => {
                eprintln!("debug1: Local forwarding listening on 127.0.0.1 port 8086.");
                std::thread::sleep(Duration::from_secs(15));
            }
            Ok("remote") => {
                eprintln!("debug1: remote forward success for: listen 8086, connect 127.0.0.1:8086");
                std::thread::sleep(Duration::from_secs(15));
            }
            Ok("network") => eprintln!("Connection reset by peer"),
            Ok("auth") => eprintln!("Permission denied (publickey)."),
            _ => panic!("missing fixture mode"),
        }
    }

    fn rule(reconnect: bool) -> SshRule {
        SshRule {
            name: "t".into(),
            ssh_target: "host".into(),
            ssh_port: None,
            identity_file: None,
            direction: Direction::Local,
            local_port: 8086,
            remote_host: "127.0.0.1".into(),
            remote_port: 8086,
            auto_reconnect: reconnect,
            enabled: true,
        }
    }

    fn fixture_child(mode: &str) -> io::Result<Child> {
        Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "forwarding::ssh::tests::fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("PORTTOOL_TEST_SSH", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
    }

    fn fixture_spawner(mode: &'static str, count: Arc<AtomicUsize>) -> Spawner {
        Arc::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            fixture_child(mode)
        })
    }

    async fn wait_state(f: &SshForwarder, want: State) -> ForwardStatus {
        for _ in 0..200 {
            let st = f.status();
            if st.state == want {
                return st;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("等待状态 {:?} 超时，当前 {:?}", want, f.status());
    }

    #[test]
    fn builds_local_forward_args() {
        let mut r = rule(false);
        r.ssh_port = Some(2222);
        r.identity_file = Some(r"C:\Users\Test User\.ssh\id_ed25519".into());
        let args = ssh_args(&r);
        let i = args.iter().position(|a| a == "-i").unwrap();
        assert_eq!(args[i + 1], r"C:\Users\Test User\.ssh\id_ed25519", "路径含空格时仍是单个参数");
        let p = args.iter().position(|a| a == "-p").unwrap();
        assert_eq!(args[p + 1], "2222");
        let l = args.iter().position(|a| a == "-L").unwrap();
        assert_eq!(args[l + 1], "127.0.0.1:8086:127.0.0.1:8086");
        assert_eq!(args.last().unwrap(), "host");
    }

    #[test]
    fn remote_direction_and_ipv6_target() {
        let mut r = rule(false);
        r.direction = Direction::Remote;
        r.remote_host = "::1".into();
        let args = ssh_args(&r);
        let i = args.iter().position(|a| a == "-R").unwrap();
        assert_eq!(args[i + 1], "127.0.0.1:8086:[::1]:8086");
    }

    #[test]
    fn retries_network_failures_but_not_auth_or_bind_errors() {
        assert!(is_transient("Connection reset by peer"));
        assert!(is_transient("ssh: connect to host example port 22: Connection timed out"));
        assert!(!is_transient("Permission denied (publickey)."));
        assert!(!is_transient("bind [127.0.0.1]:8086: Address already in use"));
        assert!(!is_transient("Host key verification failed."));
        assert!(!is_transient("Connection closed\nPermission denied"));
    }

    #[tokio::test]
    async fn reports_running_then_stops() {
        let count = Arc::new(AtomicUsize::new(0));
        let f = SshForwarder::with_spawner(rule(false), fixture_spawner("running", count));
        f.start().await.unwrap();
        let st = wait_state(&f, State::Running).await;
        assert_eq!(st.detail, "运行中");
        f.stop().await.unwrap();
        assert_eq!(f.status().state, State::Stopped);
    }

    #[tokio::test]
    async fn remote_forward_is_ready_after_server_confirmation() {
        let mut r = rule(false);
        r.direction = Direction::Remote;
        let f = SshForwarder::with_spawner(r, fixture_spawner("remote", Arc::new(AtomicUsize::new(0))));
        f.start().await.unwrap();
        wait_state(&f, State::Running).await;
        f.stop().await.unwrap();
    }

    #[tokio::test]
    async fn auth_failure_is_reported_without_retry() {
        let count = Arc::new(AtomicUsize::new(0));
        let f = SshForwarder::with_spawner(rule(true), fixture_spawner("auth", count.clone()));
        f.start().await.unwrap();
        let st = wait_state(&f, State::Failed).await;
        assert!(st.detail.contains("Permission denied"), "{}", st.detail);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn network_failure_without_auto_reconnect_fails() {
        let count = Arc::new(AtomicUsize::new(0));
        let f = SshForwarder::with_spawner(rule(false), fixture_spawner("network", count.clone()));
        f.start().await.unwrap();
        wait_state(&f, State::Failed).await;
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn transient_failure_relaunches_and_can_be_stopped() {
        let count = Arc::new(AtomicUsize::new(0));
        let attempts = count.clone();
        let spawner: Spawner = Arc::new(move |_| {
            let mode = if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                "network"
            } else {
                "running"
            };
            fixture_child(mode)
        });
        let f = SshForwarder::with_spawner(rule(true), spawner);
        f.start().await.unwrap();
        wait_state(&f, State::Running).await;
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(f.status().attempt, 0, "重连成功后不再处于重连");
        f.stop().await.unwrap();
    }

    #[tokio::test]
    async fn stop_during_backoff_cancels_relaunch() {
        let count = Arc::new(AtomicUsize::new(0));
        let f = SshForwarder::with_spawner(rule(true), fixture_spawner("network", count.clone()));
        f.start().await.unwrap();
        let st = wait_state(&f, State::Starting).await;
        // 等到进入退避阶段（详情里带“重连”）
        let mut detail = st.detail;
        for _ in 0..100 {
            if detail.contains("重连") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
            detail = f.status().detail;
        }
        assert!(detail.contains("重连"), "{detail}");
        assert_eq!(f.status().attempt, 1, "第一次退避应记为第 1 次重连");
        f.stop().await.unwrap();
        tokio::time::sleep(Duration::from_millis(2300)).await;
        assert_eq!(count.load(Ordering::SeqCst), 1, "停止后不应再拉起 ssh");
        assert_eq!(f.status().state, State::Stopped);
    }

    #[tokio::test]
    async fn spawn_failure_is_returned_from_start() {
        let spawner: Spawner = Arc::new(|_| Err(io::Error::new(io::ErrorKind::NotFound, "no ssh")));
        let f = SshForwarder::with_spawner(rule(false), spawner);
        assert!(f.start().await.is_err());
        assert_eq!(f.status().state, State::Failed);
    }

    #[tokio::test]
    async fn rejects_option_like_target() {
        let mut r = rule(false);
        r.ssh_target = "-oProxyCommand=calc".into();
        let f = SshForwarder::with_spawner(r, fixture_spawner("running", Arc::new(AtomicUsize::new(0))));
        assert!(f.start().await.is_err());
    }
}
