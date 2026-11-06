//! 服务总入口：按配置启动所有规则（串口、SSH、TCP）以及串口用到的终端服务和 HTTP 服务。

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;

use crate::config::{Config, ForwardRule, SerialRule, ServerConfig};
use crate::forwarding::{
    ssh::SshForwarder, tcp::TcpForwarder, ForwardStatus, Forwarder, RuleType, State,
};
use crate::port::{Handover, PortHub, PortSnapshot, PortState};
use crate::{http, terminal};

/// 通知外部（例如 GUI）刷新显示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceEvent {
    /// 某条规则的状态或客户端数量变化
    RuleChanged(String),
    /// 规则列表变化（重新加载配置后）
    RulesReloaded,
}

type EventSink = Arc<dyn Fn(ServiceEvent) + Send + Sync>;

struct SerialEntry {
    hub: Arc<PortHub>,
    terminal: JoinHandle<()>,
}

struct NetEntry {
    rule: ForwardRule,
    forwarder: Box<dyn Forwarder>,
}

struct HttpServer {
    task: JoinHandle<()>,
    stop_tx: oneshot::Sender<()>,
}

pub struct Service {
    config: Mutex<Config>,
    config_path: PathBuf,
    serial: Mutex<BTreeMap<String, SerialEntry>>,
    /// SSH / TCP 转发器（异步启停，所以用 tokio 的锁）
    net: RwLock<HashMap<String, NetEntry>>,
    http: Mutex<Option<HttpServer>>,
    /// 已停止的串口留下的缓冲与线程句柄，按别名索引，供同名串口再次启动时接手
    retired: Mutex<BTreeMap<String, Handover>>,
    on_event: EventSink,
}

impl Service {
    /// 启动服务。需要在 tokio 运行时中调用。
    pub async fn start(
        config: Config,
        config_path: PathBuf,
        on_event: impl Fn(ServiceEvent) + Send + Sync + 'static,
    ) -> Result<Arc<Self>> {
        config.validate()?;
        let service = Arc::new(Self {
            config: Mutex::new(config.clone()),
            config_path,
            serial: Mutex::new(BTreeMap::new()),
            net: RwLock::new(HashMap::new()),
            http: Mutex::new(None),
            retired: Mutex::new(BTreeMap::new()),
            on_event: Arc::new(on_event),
        });
        service.apply(None, config).await;
        Ok(service)
    }

    pub fn config(&self) -> Config {
        self.config.lock().unwrap().clone()
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// 按别名查串口（不区分大小写）
    pub fn port(&self, name: &str) -> Option<Arc<PortHub>> {
        let serial = self.serial.lock().unwrap();
        serial
            .values()
            .find(|e| e.hub.name().eq_ignore_ascii_case(name))
            .map(|e| e.hub.clone())
    }

    pub fn ports(&self) -> Vec<Arc<PortHub>> {
        self.serial.lock().unwrap().values().map(|e| e.hub.clone()).collect()
    }

    pub fn snapshots(&self) -> Vec<PortSnapshot> {
        self.ports().iter().map(|h| h.snapshot()).collect()
    }

    /// 所有规则的状态：串口在前，其后是 SSH / TCP（各自按名称排序）；
    /// 已停用的规则也在列表里，状态为 `Stopped`。
    pub async fn list_rules(&self) -> Vec<ForwardStatus> {
        let mut out: Vec<ForwardStatus> = self.snapshots().iter().map(serial_status).collect();
        {
            let net = self.net.read().await;
            out.extend(net.values().map(|e| e.forwarder.status()));
        }
        let config = self.config();
        for rule in config.rules.iter().filter(|r| !r.enabled()) {
            out.push(ForwardStatus {
                name: rule.name().to_string(),
                rule_type: rule.rule_type(),
                state: State::Stopped,
                detail: "已停用".into(),
                clients: 0,
            });
        }
        out.sort_by(|a, b| {
            (a.rule_type != RuleType::Serial, &a.name).cmp(&(b.rule_type != RuleType::Serial, &b.name))
        });
        out
    }

    /// 启用 / 停用一条规则并**保存到配置文件**（串口、SSH、TCP 都适用）。
    ///
    /// 以磁盘上的配置为基准（文件不存在时用内存里的配置），这样手工改过但还没
    /// 重新加载的内容不会被冲掉；保存后立即应用，规则随之启动或停止。
    pub async fn set_enabled(self: &Arc<Self>, name: &str, enabled: bool) -> Result<()> {
        let mut config = if self.config_path.exists() {
            Config::load(&self.config_path)?
        } else {
            self.config()
        };
        let rule = config
            .rules
            .iter_mut()
            .find(|r| r.name() == name)
            .ok_or_else(|| anyhow::anyhow!("规则不存在: {name}"))?;
        match rule {
            ForwardRule::Serial(r) => r.enabled = enabled,
            ForwardRule::Ssh(r) => r.enabled = enabled,
            ForwardRule::Tcp(r) => r.enabled = enabled,
        }
        config.save(&self.config_path)?;
        self.reload(config).await
    }

    /// 应用新配置：配置没变的规则保持运行（终端连接不断），其余的停止或新建。
    pub async fn reload(self: &Arc<Self>, config: Config) -> Result<()> {
        config.validate()?;
        let old = std::mem::replace(&mut *self.config.lock().unwrap(), config.clone());
        self.apply(Some(&old.server), config).await;
        (self.on_event)(ServiceEvent::RulesReloaded);
        Ok(())
    }

    /// `old` 为 `None` 表示首次启动。
    async fn apply(self: &Arc<Self>, old: Option<&ServerConfig>, config: Config) {
        self.apply_serial(old, &config);
        self.apply_net(&config).await;
    }

    fn apply_serial(self: &Arc<Self>, old: Option<&ServerConfig>, config: &Config) {
        let server = &config.server;
        // token 每个请求都现读配置，改它不需要重启任何东西。
        let http_changed = old.is_none_or(|o| o.bind != server.bind || o.http_port != server.http_port);
        // 终端端口跟随 bind，日志文件由 IO 线程持有，这两项变了要重建串口。
        let ports_changed = old.is_some_and(|o| o.bind != server.bind || o.log_dir != server.log_dir);

        if http_changed {
            warn_if_exposed(config);
            let (stop_tx, stop_rx) = oneshot::channel();
            let task = tokio::spawn(http::serve(
                self.clone(),
                server.bind.clone(),
                server.http_port,
                stop_rx,
            ));
            if let Some(old) = self.http.lock().unwrap().replace(HttpServer { task, stop_tx }) {
                // 优雅关闭：旧监听立即停止接受连接，已建立的连接处理完当前请求后关闭。
                let _ = old.stop_tx.send(());
            }
        }

        let log_root = config.log_dir(&self.config_path);
        let wanted: BTreeMap<String, SerialRule> = config
            .serial_rules()
            .filter(|p| p.enabled)
            .map(|p| (p.name.clone(), p.clone()))
            .collect();

        // 锁内只摘出要停的条目，停止和等待都在锁外做。
        let stale: Vec<SerialEntry> = {
            let mut serial = self.serial.lock().unwrap();
            let names: Vec<String> = serial
                .iter()
                .filter(|(name, entry)| ports_changed || wanted.get(*name) != Some(entry.hub.config()))
                .map(|(name, _)| name.clone())
                .collect();
            names.iter().filter_map(|n| serial.remove(n)).collect()
        };
        // 停下的串口留下缓冲和线程句柄：同名串口（这次重建，或以后停用后再启用、删掉后再加回）
        // 接着用同一份缓冲，seq 连续；并先等旧线程关闭串口再打开，避免"拒绝访问"。
        let mut retired = self.retired.lock().unwrap();
        for entry in stale {
            entry.terminal.abort();
            // 不在这里等待旧线程，由同名的新线程自己等。
            let predecessor = entry.hub.request_stop();
            retired.insert(
                entry.hub.name().to_string(),
                Handover { buffer: Some(entry.hub.buffer_handle()), predecessor },
            );
        }

        let mut serial = self.serial.lock().unwrap();
        for entry in serial.values() {
            entry.hub.with_buffer_mut(|b| b.set_capacity(server.buffer_lines));
        }
        for (name, rule) in wanted {
            if serial.contains_key(&name) {
                continue;
            }
            let on_event = self.on_event.clone();
            let handover = retired.remove(&name).unwrap_or_default();
            let hub = PortHub::start(rule, server.buffer_lines, &log_root, handover, move |n| {
                on_event(ServiceEvent::RuleChanged(n.to_string()))
            });
            let terminal = tokio::spawn(terminal::serve(hub.clone(), server.bind.clone()));
            serial.insert(name, SerialEntry { hub, terminal });
        }
    }

    async fn apply_net(&self, config: &Config) {
        let mut net = self.net.write().await;

        // 停掉被删除、被停用或参数变化的规则
        let stale: Vec<String> = net
            .iter()
            .filter(|(name, entry)| {
                !config
                    .rules
                    .iter()
                    .any(|r| r.name() == name.as_str() && r.enabled() && *r == entry.rule)
            })
            .map(|(name, _)| name.clone())
            .collect();
        for name in stale {
            if let Some(entry) = net.remove(&name) {
                if let Err(e) = entry.forwarder.stop().await {
                    tracing::warn!("停止规则 {name} 失败: {e}");
                }
            }
        }

        for rule in &config.rules {
            if !rule.enabled() || net.contains_key(rule.name()) {
                continue;
            }
            let forwarder: Box<dyn Forwarder> = match rule {
                ForwardRule::Serial(_) => continue,
                ForwardRule::Ssh(r) => Box::new(SshForwarder::new(r.clone())),
                ForwardRule::Tcp(r) => Box::new(TcpForwarder::new(r.clone())),
            };
            let name = rule.name().to_string();
            match forwarder.start().await {
                Ok(()) => tracing::info!("规则 {name} 已启动"),
                // 启动失败的规则仍保留在列表里，状态为 Failed，用户可在界面上看到原因
                Err(e) => tracing::error!("规则 {name} 启动失败: {e}"),
            }
            net.insert(name, NetEntry { rule: rule.clone(), forwarder });
        }
    }

    /// 停止所有规则、HTTP 服务，并等待串口线程退出。
    pub async fn shutdown(&self) {
        if let Some(http) = self.http.lock().unwrap().take() {
            http.task.abort();
        }
        let entries = std::mem::take(&mut *self.serial.lock().unwrap());
        let handles: Vec<_> = entries
            .into_values()
            .filter_map(|entry| {
                entry.terminal.abort();
                entry.hub.request_stop()
            })
            .collect();
        let retired = std::mem::take(&mut *self.retired.lock().unwrap());
        let retired_handles: Vec<_> = retired.into_values().filter_map(|h| h.predecessor).collect();

        let mut net = self.net.write().await;
        for (name, entry) in net.drain() {
            if let Err(e) = entry.forwarder.stop().await {
                tracing::warn!("停止规则 {name} 失败: {e}");
            }
        }
        drop(net);

        // join 会阻塞，放到阻塞线程池里做，避免卡住异步运行时
        let _ = tokio::task::spawn_blocking(move || {
            for handle in handles.into_iter().chain(retired_handles) {
                let _ = handle.join();
            }
        })
        .await;
    }
}

fn serial_status(s: &PortSnapshot) -> ForwardStatus {
    let (state, detail) = match &s.state {
        PortState::Waiting => (State::Starting, "等待设备接入".to_string()),
        PortState::Connected { com } => (State::Running, format!("已连接 {com} @ {}", s.baud)),
        PortState::Error(e) => (State::Failed, e.clone()),
    };
    ForwardStatus {
        name: s.name.clone(),
        rule_type: RuleType::Serial,
        state,
        detail,
        clients: s.clients,
    }
}

/// 对外监听时提醒当前的访问控制情况。
fn warn_if_exposed(config: &Config) {
    let loopback = config
        .server
        .bind
        .parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false);
    if loopback {
        return;
    }
    if config.server.token.is_none() {
        tracing::warn!(
            bind = %config.server.bind,
            "服务对局域网开放且未设置 token，任何能访问本机的设备都可以读写串口"
        );
    } else {
        tracing::warn!(
            bind = %config.server.bind,
            "token 只保护 HTTP 接口；终端端口（Telnet/Raw）不做认证，局域网内可以直接连接"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Encoding, TcpRule, TerminalMode};

    fn serial(name: &str, tcp_port: u16, baud: u32) -> ForwardRule {
        ForwardRule::Serial(SerialRule {
            name: name.into(),
            serial_number: None,
            // 不存在的串口：服务会一直处于等待状态，测试不依赖硬件
            com: Some("COM250".into()),
            baud,
            tcp_port,
            mode: TerminalMode::Telnet,
            encoding: Encoding::Utf8,
            enabled: true,
        })
    }

    fn tcp(name: &str, listen: &str, target: &str) -> ForwardRule {
        ForwardRule::Tcp(TcpRule {
            name: name.into(),
            listen: listen.into(),
            target: target.into(),
            max_connections: 100,
            enabled: true,
        })
    }

    fn config(rules: Vec<ForwardRule>) -> Config {
        Config {
            server: ServerConfig {
                bind: "127.0.0.1".into(),
                http_port: 47100,
                ..ServerConfig::default()
            },
            rules,
        }
    }

    async fn free_addr() -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().to_string()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn serial_reload_keeps_unchanged_ports_and_reuses_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let service = Service::start(config(vec![serial("a", 47101, 115_200)]), path, |_| {})
            .await
            .unwrap();
        let a = service.port("a").unwrap();

        service
            .reload(config(vec![serial("a", 47101, 115_200), serial("b", 47102, 115_200)]))
            .await
            .unwrap();
        assert!(Arc::ptr_eq(&a, &service.port("A").unwrap()), "未变化的串口应保持运行");
        let b = service.port("b").unwrap();
        b.with_buffer_mut(|buf| {
            buf.push(chrono::Local::now(), crate::buffer::Direction::Rx, "x".into());
        });
        let seq_before = b.snapshot().last_seq;

        // 只改 token：串口不重建
        let mut with_token = config(vec![serial("a", 47101, 115_200), serial("b", 47102, 115_200)]);
        with_token.server.token = Some("t".into());
        service.reload(with_token).await.unwrap();
        assert!(Arc::ptr_eq(&b, &service.port("b").unwrap()), "只改 token 不应重建串口");

        service.reload(config(vec![serial("b", 47102, 921_600)])).await.unwrap();
        assert!(service.port("a").is_none(), "删除的串口应停止");
        let b2 = service.port("b").unwrap();
        assert!(!Arc::ptr_eq(&b, &b2), "配置变化的串口应重新创建");
        assert_eq!(b2.config().baud, 921_600);
        assert!(Arc::ptr_eq(&b.buffer_handle(), &b2.buffer_handle()), "重建应沿用旧缓冲");
        assert!(b2.snapshot().last_seq >= seq_before, "重建后 seq 不应回退");

        let mut disabled = serial("b", 47102, 921_600);
        if let ForwardRule::Serial(s) = &mut disabled {
            s.enabled = false;
        }
        service.reload(config(vec![disabled])).await.unwrap();
        assert!(service.ports().is_empty(), "停用的串口不应运行");

        service.reload(config(vec![serial("b", 47102, 921_600)])).await.unwrap();
        let b3 = service.port("b").unwrap();
        assert!(Arc::ptr_eq(&b2.buffer_handle(), &b3.buffer_handle()), "重新启用应沿用旧缓冲");

        assert!(
            service.reload(config(vec![serial("x", 47100, 1)])).await.is_err(),
            "与 HTTP 端口冲突应拒绝"
        );
        service.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn mixed_rules_are_listed_and_tcp_restarts_on_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let listen = free_addr().await;
        let service = Service::start(
            config(vec![serial("com1", 47121, 115_200), tcp("web", &listen, "127.0.0.1:9")]),
            path,
            |_| {},
        )
        .await
        .unwrap();

        let rules = service.list_rules().await;
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].rule_type, RuleType::Serial);
        assert_eq!(rules[0].state, State::Starting, "没有设备时串口处于等待");
        assert_eq!(rules[1].rule_type, RuleType::Tcp);
        assert_eq!(rules[1].state, State::Running);

        // 目标地址变化：同名 TCP 规则应被重建
        let listen2 = free_addr().await;
        service
            .reload(config(vec![
                serial("com1", 47121, 115_200),
                tcp("web", &listen2, "127.0.0.1:10"),
            ]))
            .await
            .unwrap();
        let web = service.list_rules().await.into_iter().find(|r| r.name == "web").unwrap();
        assert_eq!(web.state, State::Running);
        assert!(web.detail.contains(&listen2), "应监听新地址: {}", web.detail);

        // 停用 / 启用 TCP 规则：持久化到配置文件，停用后仍在列表里（Stopped）
        let path = service.config_path().to_path_buf();
        service.set_enabled("web", false).await.unwrap();
        let web = service.list_rules().await.into_iter().find(|r| r.name == "web").unwrap();
        assert_eq!(web.state, State::Stopped);
        assert_eq!(web.rule_type, RuleType::Tcp);
        assert!(!Config::load(&path).unwrap().rules.iter().find(|r| r.name() == "web").unwrap().enabled());
        service.set_enabled("web", true).await.unwrap();
        let web = service.list_rules().await.into_iter().find(|r| r.name == "web").unwrap();
        assert_eq!(web.state, State::Running);
        assert!(Config::load(&path).unwrap().rules.iter().find(|r| r.name() == "web").unwrap().enabled());

        // 串口规则同样适用：停用后串口停止，但仍列出
        service.set_enabled("com1", false).await.unwrap();
        assert!(service.port("com1").is_none(), "停用后串口应停止");
        let rules = service.list_rules().await;
        let com = rules.iter().find(|r| r.name == "com1").unwrap();
        assert_eq!((com.rule_type, com.state), (RuleType::Serial, State::Stopped));
        assert_eq!(rules[0].name, "com1", "串口排在最前");
        service.set_enabled("com1", true).await.unwrap();
        assert!(service.port("com1").is_some());

        assert!(service.set_enabled("nope", true).await.is_err());

        service.shutdown().await;
        assert!(service.list_rules().await.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn set_enabled_rejects_broken_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let service = Service::start(config(vec![tcp("web", &free_addr().await, "127.0.0.1:9")]), path.clone(), |_| {})
            .await
            .unwrap();
        std::fs::write(&path, "this is = not [valid toml").unwrap();
        assert!(service.set_enabled("web", false).await.is_err(), "配置文件损坏时不应覆盖它");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "this is = not [valid toml");
        service.shutdown().await;
    }
}
