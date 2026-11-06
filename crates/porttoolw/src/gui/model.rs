//! 视图模型：把配置 + 运行状态整理成界面直接显示的行和统计。纯函数，不依赖 windui。

use std::collections::HashMap;

use porttool_core::config::{Config, Direction, Encoding, ForwardRule, TerminalMode};
use porttool_core::forwarding::{ForwardStatus, RuleType, State};
use porttool_core::port::PortSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Serial,
    Ssh,
    Tcp,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Serial => "串口",
            Kind::Ssh => "SSH",
            Kind::Tcp => "TCP",
        }
    }
}

/// 状态胶囊的语气，对应颜色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Warn,
    Bad,
    Off,
}

/// 规则表格里的一行。
#[derive(Debug, Clone, PartialEq)]
pub struct RuleRow {
    pub kind: Kind,
    pub name: String,
    /// 路径主行，等宽字体，例如 `COM5 · 1500000 → :7001`
    pub path: String,
    /// 路径副行，例如 `telnet · utf-8`
    pub sub: String,
    pub status: String,
    pub tone: Tone,
    /// 状态详情（失败原因、重连倒计时等）
    pub detail: String,
    pub enabled: bool,
    /// SSH 规则的方向标签，例如 `本地 -L`
    pub tag: String,
    /// 三段流程：本机监听 → 中间 → 目标（SSH / TCP 规则用）
    pub from: String,
    pub via: String,
    pub to: String,
    /// 串口规则的终端端口（其它规则为 0）
    pub port: u16,
}

/// 每次刷新都可能变化的数据（连接数、流量）。
///
/// 与结构性的 [`RuleRow`] 分开：行进入 `list_signal` 后，数据一变就会整行重建（丢失悬停、
/// 菜单锚点、开关动画）；这些高频数据改用每条规则一组文案信号绑定，行就不会因它们而重建。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Metrics {
    /// 当前连接数；规则没在运行或没有该数据时为 `None`
    pub clients: Option<usize>,
    /// 串口累计接收 / 发送字节数
    pub traffic: Option<(u64, u64)>,
}

impl Metrics {
    /// `2 个连接`，没有数据时 `—`
    pub fn conns_text(&self) -> String {
        self.clients.map_or_else(|| "—".into(), |n| format!("{n} 个连接"))
    }

    pub fn clients_text(&self) -> String {
        self.clients.unwrap_or(0).to_string()
    }

    /// `↓ 27.3 MB ↑ 9.0 KB`，没有数据时 `—`
    pub fn traffic_text(&self) -> String {
        self.traffic
            .map_or_else(|| "—".into(), |(rx, tx)| format!("↓ {} ↑ {}", human_bytes(rx), human_bytes(tx)))
    }

    pub fn rx_text(&self) -> String {
        self.traffic.map_or_else(|| "—".into(), |(rx, _)| human_bytes(rx))
    }

    pub fn tx_text(&self) -> String {
        self.traffic.map_or_else(|| "—".into(), |(_, tx)| human_bytes(tx))
    }
}

/// 概览页统计卡片。
#[derive(Debug, Clone, PartialEq)]
pub struct KindStat {
    pub online: usize,
    pub total: usize,
    /// 右上角的短评，例如 `全部正常` / `1 个异常`
    pub note: String,
    pub tone: Tone,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub serial: KindStat,
    pub ssh: KindStat,
    pub tcp: KindStat,
    pub connections: usize,
}

pub fn human_bytes(n: u64) -> String {
    match n {
        0..1024 => format!("{n} B"),
        1024..1_048_576 => format!("{:.1} KB", n as f64 / 1024.0),
        _ => format!("{:.1} MB", n as f64 / 1_048_576.0),
    }
}

fn mode_name(mode: TerminalMode) -> &'static str {
    match mode {
        TerminalMode::Telnet => "telnet",
        TerminalMode::Raw => "raw",
    }
}

fn encoding_name(e: Encoding) -> &'static str {
    match e {
        Encoding::Utf8 => "utf-8",
        Encoding::Gbk => "gbk",
    }
}

/// 把配置里的每条规则对上它的运行状态，生成表格行（顺序与配置一致）。
pub fn build_rows(config: &Config, statuses: &[ForwardStatus]) -> Vec<RuleRow> {
    config
        .rules
        .iter()
        .map(|rule| {
            let status = statuses
                .iter()
                .find(|s| s.name == rule.name() && s.rule_type == rule_type(rule));
            let mut row = match rule {
                ForwardRule::Serial(r) => {
                    let target = r
                        .com
                        .clone()
                        .or_else(|| r.serial_number.as_ref().map(|sn| format!("SN {sn}")))
                        .unwrap_or_else(|| "—".into());
                    // 已连接时显示实际的 COM 号（序列号绑定的设备 COM 号可能变化）
                    let com = status.and_then(|s| s.endpoint.clone()).unwrap_or(target);
                    RuleRow {
                        kind: Kind::Serial,
                        name: r.name.clone(),
                        path: format!("{com} · {} → :{}", r.baud, r.tcp_port),
                        sub: format!("{} · {}", mode_name(r.mode), encoding_name(r.encoding)),
                        from: format!("{com} · {}", r.baud),
                        via: mode_name(r.mode).to_string(),
                        to: format!("{} :{}", mode_name(r.mode), r.tcp_port),
                        port: r.tcp_port,
                        ..blank(Kind::Serial, &r.name, r.enabled)
                    }
                }
                ForwardRule::Ssh(r) => {
                    let (path, how) = match r.direction {
                        Direction::Local => (
                            format!("127.0.0.1:{} → {}:{}", r.local_port, r.remote_host, r.remote_port),
                            "本地转发",
                        ),
                        Direction::Remote => (
                            format!("远端 :{} → {}:{}", r.local_port, r.remote_host, r.remote_port),
                            "远程转发",
                        ),
                    };
                    let server = match r.ssh_port {
                        Some(p) => format!("{}:{p}", r.ssh_target),
                        None => r.ssh_target.clone(),
                    };
                    let (tag, from) = match r.direction {
                        Direction::Local => ("本地 -L", format!("127.0.0.1:{}", r.local_port)),
                        Direction::Remote => ("远程 -R", format!("远端 127.0.0.1:{}", r.local_port)),
                    };
                    RuleRow {
                        path,
                        sub: format!("{how} · 经 {}", r.ssh_target),
                        tag: tag.into(),
                        from,
                        via: server,
                        to: format!("{}:{}", r.remote_host, r.remote_port),
                        ..blank(Kind::Ssh, &r.name, r.enabled)
                    }
                }
                ForwardRule::Tcp(r) => RuleRow {
                    path: format!("{} → {}", r.listen, r.target),
                    sub: format!("TCP · 最多 {} 个连接", r.max_connections),
                    from: r.listen.clone(),
                    to: r.target.clone(),
                    ..blank(Kind::Tcp, &r.name, r.enabled)
                },
            };

            match (row.enabled, status) {
                (false, _) | (true, None) => {
                    row.status = if row.enabled { "启动中".into() } else { "已停用".into() };
                    row.tone = Tone::Off;
                }
                (true, Some(s)) => {
                    let (text, tone) = state_text(row.kind, s);
                    row.status = text;
                    row.tone = tone;
                    row.detail = s.detail.clone();
                }
            }
            row
        })
        .collect()
}

/// 每条规则当前的连接数和流量（见 [`Metrics`]）；键是规则名。
///
/// 连接数只有「已启用且在运行」的串口 / TCP 规则才有；流量只有串口有，来自串口快照。
pub fn build_metrics(
    config: &Config,
    statuses: &[ForwardStatus],
    snapshots: &[PortSnapshot],
) -> HashMap<String, Metrics> {
    config
        .rules
        .iter()
        .map(|rule| {
            let status = statuses
                .iter()
                .find(|s| s.name == rule.name() && s.rule_type == rule_type(rule));
            let clients = match (rule, status) {
                (ForwardRule::Ssh(_), _) | (_, None) => None,
                (_, Some(s)) => (rule.enabled() && s.state == State::Running).then_some(s.clients),
            };
            let traffic = match rule {
                ForwardRule::Serial(r) => snapshots
                    .iter()
                    .find(|s| s.name == r.name)
                    .map(|s| (s.rx_bytes, s.tx_bytes)),
                _ => None,
            };
            (rule.name().to_string(), Metrics { clients, traffic })
        })
        .collect()
}

fn rule_type(rule: &ForwardRule) -> RuleType {
    match rule {
        ForwardRule::Serial(_) => RuleType::Serial,
        ForwardRule::Ssh(_) => RuleType::Ssh,
        ForwardRule::Tcp(_) => RuleType::Tcp,
    }
}

fn blank(kind: Kind, name: &str, enabled: bool) -> RuleRow {
    RuleRow {
        kind,
        name: name.to_string(),
        path: String::new(),
        sub: String::new(),
        status: String::new(),
        tone: Tone::Off,
        detail: String::new(),
        enabled,
        tag: String::new(),
        from: String::new(),
        via: String::new(),
        to: String::new(),
        port: 0,
    }
}

fn state_text(kind: Kind, s: &ForwardStatus) -> (String, Tone) {
    match (kind, s.state) {
        (Kind::Serial, State::Running) => ("已连接".into(), Tone::Ok),
        (Kind::Serial, State::Starting) => ("等待设备".into(), Tone::Warn),
        (Kind::Ssh, State::Running) => ("运行中".into(), Tone::Ok),
        (Kind::Ssh, State::Starting) if s.attempt > 0 => (format!("重连中 第{}次", s.attempt), Tone::Warn),
        (Kind::Tcp, State::Running) => ("监听中".into(), Tone::Ok),
        (_, State::Starting) => ("连接中".into(), Tone::Warn),
        (Kind::Tcp, State::Failed) => ("监听失败".into(), Tone::Bad),
        (_, State::Failed) => ("失败".into(), Tone::Bad),
        (_, State::Stopped) => ("已停止".into(), Tone::Off),
    }
}

pub fn stats(rows: &[RuleRow], metrics: &HashMap<String, Metrics>) -> Stats {
    let kind_stat = |kind: Kind| {
        let mine: Vec<&RuleRow> = rows.iter().filter(|r| r.kind == kind).collect();
        let online = mine.iter().filter(|r| r.tone == Tone::Ok).count();
        let bad = mine.iter().filter(|r| r.tone == Tone::Bad).count();
        let warn = mine.iter().filter(|r| r.tone == Tone::Warn).count();
        let (note, tone) = if bad > 0 {
            (format!("{bad} 个异常"), Tone::Bad)
        } else if warn > 0 {
            let word = match kind {
                Kind::Serial => "等待设备",
                _ => "处理中",
            };
            (format!("{warn} 个{word}"), Tone::Warn)
        } else if mine.is_empty() {
            ("未配置".into(), Tone::Off)
        } else if online == mine.iter().filter(|r| r.enabled).count() {
            ("全部正常".into(), Tone::Ok)
        } else {
            (String::new(), Tone::Off)
        };
        KindStat { online, total: mine.len(), note, tone }
    };
    let connections = rows
        .iter()
        .filter(|r| r.kind != Kind::Ssh)
        .filter_map(|r| metrics.get(&r.name)?.clients)
        .sum();
    Stats {
        serial: kind_stat(Kind::Serial),
        ssh: kind_stat(Kind::Ssh),
        tcp: kind_stat(Kind::Tcp),
        connections,
    }
}

/// 顶部状态胶囊：`4/7 在线 · 2 需处理`。
pub fn summary(rows: &[RuleRow]) -> (String, bool) {
    let online = rows.iter().filter(|r| r.tone == Tone::Ok).count();
    let attention = rows.iter().filter(|r| matches!(r.tone, Tone::Bad | Tone::Warn)).count();
    let text = if attention > 0 {
        format!("{online}/{} 在线 · {attention} 需处理", rows.len())
    } else {
        format!("{online}/{} 在线", rows.len())
    };
    (text, attention > 0)
}

/// 失败的规则：告警条用。
pub fn failures(rows: &[RuleRow]) -> Vec<&RuleRow> {
    rows.iter().filter(|r| r.tone == Tone::Bad).collect()
}

/// 一条失败的标识：规则名 + 失败原因。原因变了就是新的失败，需要重新提醒。
pub fn alert_key(row: &RuleRow) -> String {
    format!("{}/{}|{}", row.kind.label(), row.name, row.detail)
}

/// 告警条要显示的失败：去掉用户已经关掉的；顺便把已不再失败（规则恢复或被删）的条目
/// 从 `dismissed` 里清掉，这样同一条规则以后再失败时会重新提醒。
pub fn visible_alerts(
    rows: &[RuleRow],
    dismissed: &mut std::collections::HashSet<String>,
) -> Vec<RuleRow> {
    let failing: Vec<(String, &RuleRow)> = failures(rows).into_iter().map(|r| (alert_key(r), r)).collect();
    dismissed.retain(|key| failing.iter().any(|(k, _)| k == key));
    failing
        .into_iter()
        .filter(|(key, _)| !dismissed.contains(key))
        .map(|(_, r)| r.clone())
        .collect()
}

/// 从 ssh 目标推断规则名：取别名或主机段（去掉 user@、端口和路径），小写，
/// 规则名不允许的字符（点、空格等）按词折叠成 `-`。
pub fn suggested_name(target: &str, existing: &[String]) -> String {
    let host = target.trim().split('@').next_back().unwrap_or("");
    let host = host.split([':', '/']).next().unwrap_or(host);
    let words: Vec<String> = host
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect();
    let base = if words.is_empty() { "ssh".into() } else { words.join("-") };
    unique_name(&base, existing)
}

/// 与现有规则名（不区分大小写）不重复的名字；重名依次加 `-2`、`-3`。
pub fn unique_name(base: &str, existing: &[String]) -> String {
    (1..)
        .map(|i| if i == 1 { base.to_string() } else { format!("{base}-{i}") })
        .find(|n| !existing.iter().any(|e| e.eq_ignore_ascii_case(n)))
        .unwrap_or_else(|| format!("{base}-n"))
}

/// 新规则的候选监听端口：从已用端口最大值 +1（且至少 7001）开始，给出一段候选
/// （探测可绑定时逐个试，见 editors 的 `first_free_port`）。
pub fn next_candidate_ports(used: &[u16]) -> Vec<u16> {
    let start = used
        .iter()
        .copied()
        .max()
        .map_or(7_001, |p| p.saturating_add(1))
        .max(7_001) as u32;
    (start..)
        .take(64)
        .take_while(|p| *p <= u16::MAX as u32)
        .filter_map(|p| u16::try_from(p).ok())
        .collect()
}

/// 按类型和关键字筛选；关键字匹配名称、路径、副行（不区分大小写）。
pub fn filter_rows(rows: &[RuleRow], kind: Option<Kind>, query: &str) -> Vec<RuleRow> {
    let q = query.trim().to_lowercase();
    rows.iter()
        .filter(|r| kind.is_none_or(|k| r.kind == k))
        .filter(|r| {
            q.is_empty()
                || [&r.name, &r.path, &r.sub]
                    .iter()
                    .any(|f| f.to_lowercase().contains(&q))
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use porttool_core::config::{SerialRule, SshRule, TcpRule};

    fn config() -> Config {
        Config {
            rules: vec![
                ForwardRule::Serial(SerialRule {
                    name: "A1".into(),
                    serial_number: None,
                    com: Some("COM5".into()),
                    baud: 1_500_000,
                    tcp_port: 7001,
                    mode: TerminalMode::Telnet,
                    encoding: Encoding::Utf8,
                    enabled: true,
                }),
                ForwardRule::Ssh(SshRule {
                    name: "gitlab".into(),
                    ssh_target: "ops@jump".into(),
                    ssh_port: None,
                    identity_file: None,
                    direction: Direction::Local,
                    local_port: 8443,
                    remote_host: "gitlab.internal".into(),
                    remote_port: 443,
                    auto_reconnect: true,
                    enabled: true,
                }),
                ForwardRule::Tcp(TcpRule {
                    name: "mqtt".into(),
                    listen: "0.0.0.0:1883".into(),
                    target: "192.168.5.60:1883".into(),
                    max_connections: 1000,
                    enabled: false,
                }),
            ],
            ..Config::default()
        }
    }

    fn status(name: &str, rule_type: RuleType, state: State, detail: &str, clients: usize) -> ForwardStatus {
        ForwardStatus {
            name: name.into(),
            rule_type,
            state,
            detail: detail.into(),
            clients,
            endpoint: None,
            attempt: 0,
        }
    }

    #[test]
    fn rows_combine_config_and_status() {
        let statuses = vec![
            ForwardStatus {
                endpoint: Some("COM7".into()),
                ..status("A1", RuleType::Serial, State::Running, "已连接 COM7 @ 1500000", 2)
            },
            ForwardStatus {
                attempt: 2,
                ..status("gitlab", RuleType::Ssh, State::Starting, "连接中断，4 秒后重连：timeout", 0)
            },
        ];
        let rows = build_rows(&config(), &statuses);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].path, "COM7 · 1500000 → :7001", "已连接时显示实际 COM 号");
        assert_eq!(rows[0].status, "已连接");
        assert_eq!(rows[1].status, "重连中 第2次", "重连次数来自结构化字段，不解析 detail 文字");
        assert_eq!(rows[1].tone, Tone::Warn);
        assert_eq!(rows[1].path, "127.0.0.1:8443 → gitlab.internal:443");
        assert_eq!(rows[2].status, "已停用");
        assert!(!rows[2].enabled);
    }

    #[test]
    fn reconnect_state_does_not_depend_on_detail_text() {
        // 文字里带「重连」但 attempt 为 0：只是普通的「连接中」
        let first = status("gitlab", RuleType::Ssh, State::Starting, "正在重连…", 0);
        assert_eq!(state_text(Kind::Ssh, &first), ("连接中".to_string(), Tone::Warn));
        // 串口没有 endpoint 时用配置里的设备
        let serial = status("A1", RuleType::Serial, State::Starting, "已连接 COM9 @ 1", 0);
        let rows = build_rows(&config(), &[serial]);
        assert_eq!(rows[0].path, "COM5 · 1500000 → :7001", "COM 号只认 endpoint，不再解析 detail");
    }

    fn snapshot(name: &str, rx: u64, tx: u64) -> PortSnapshot {
        use porttool_core::port::PortState;
        PortSnapshot {
            name: name.into(),
            state: PortState::Waiting,
            baud: 1_500_000,
            tcp_port: 7001,
            mode: TerminalMode::Telnet,
            clients: 2,
            last_seq: 0,
            rx_bytes: rx,
            tx_bytes: tx,
        }
    }

    #[test]
    fn metrics_hold_connections_and_traffic_apart_from_rows() {
        let statuses = vec![
            status("A1", RuleType::Serial, State::Running, "已连接 COM7 @ 1500000", 2),
            status("gitlab", RuleType::Ssh, State::Running, "运行中", 0),
        ];
        let metrics = build_metrics(&config(), &statuses, &[snapshot("A1", 3 * 1_048_576, 2048)]);
        assert_eq!(metrics["A1"].conns_text(), "2 个连接");
        assert_eq!(metrics["A1"].traffic_text(), "↓ 3.0 MB ↑ 2.0 KB");
        assert_eq!((metrics["A1"].rx_text(), metrics["A1"].tx_text()), ("3.0 MB".into(), "2.0 KB".into()));
        assert_eq!(metrics["gitlab"].conns_text(), "—", "SSH 没有连接数");
        assert_eq!(metrics["mqtt"], Metrics::default(), "已停用的规则没有数据");
        assert_eq!(metrics["mqtt"].traffic_text(), "—");

        // 流量变了只体现在 Metrics 上；build_rows 根本不接收快照，行不会受影响
        let metrics_b = build_metrics(&config(), &statuses, &[snapshot("A1", 9, 9)]);
        assert_ne!(metrics["A1"], metrics_b["A1"]);
    }

    #[test]
    fn stats_summary_and_failures() {
        let statuses = vec![
            status("A1", RuleType::Serial, State::Running, "已连接 COM5 @ 1", 2),
            status("gitlab", RuleType::Ssh, State::Failed, "Permission denied", 0),
        ];
        let rows = build_rows(&config(), &statuses);
        let metrics = build_metrics(&config(), &statuses, &[]);
        let st = stats(&rows, &metrics);
        assert_eq!((st.serial.online, st.serial.total), (1, 1));
        assert_eq!(st.serial.note, "全部正常");
        assert_eq!(st.ssh.tone, Tone::Bad);
        assert_eq!(st.connections, 2);
        let (text, attention) = summary(&rows);
        assert_eq!(text, "1/3 在线 · 1 需处理");
        assert!(attention);
        assert_eq!(failures(&rows).len(), 1);
    }

    #[test]
    fn dismissed_alerts_stay_hidden_until_the_failure_changes_or_recurs() {
        let failed = |detail: &str| {
            build_rows(
                &config(),
                &[status("gitlab", RuleType::Ssh, State::Failed, detail, 0)],
            )
        };
        let mut dismissed = std::collections::HashSet::new();

        // 第一次失败：要提醒；关掉后不再显示
        let rows = failed("Permission denied");
        let shown = visible_alerts(&rows, &mut dismissed);
        assert_eq!(shown.len(), 1);
        dismissed.insert(alert_key(&shown[0]));
        assert!(visible_alerts(&rows, &mut dismissed).is_empty());

        // 原因变了：是新的失败，重新提醒
        let rows = failed("Connection refused");
        assert_eq!(visible_alerts(&rows, &mut dismissed).len(), 1);
        assert!(dismissed.is_empty(), "旧的已不再出现，应被清理");

        // 关掉后恢复，之后以同样的原因再失败：重新提醒
        let shown = visible_alerts(&rows, &mut dismissed);
        dismissed.insert(alert_key(&shown[0]));
        let healthy = build_rows(&config(), &[status("gitlab", RuleType::Ssh, State::Running, "运行中", 0)]);
        assert!(visible_alerts(&healthy, &mut dismissed).is_empty());
        assert_eq!(visible_alerts(&rows, &mut dismissed).len(), 1);
    }

    #[test]
    fn serial_row_carries_what_the_flow_blocks_show() {
        let rows = build_rows(&config(), &[]);
        let a1 = rows.iter().find(|r| r.name == "A1").unwrap();
        assert_eq!(a1.from, "COM5 · 1500000");
        assert_eq!(a1.to, "telnet :7001");
        assert_eq!((a1.via.as_str(), a1.port), ("telnet", 7001));
        // 非串口规则没有终端端口
        assert!(rows.iter().filter(|r| r.kind != Kind::Serial).all(|r| r.port == 0));
    }

    #[test]
    fn filter_by_kind_and_query() {
        let rows = build_rows(&config(), &[]);
        assert_eq!(filter_rows(&rows, None, "").len(), 3);
        assert_eq!(filter_rows(&rows, Some(Kind::Ssh), "").len(), 1);
        assert_eq!(filter_rows(&rows, None, "GITLAB").len(), 1);
        assert_eq!(filter_rows(&rows, None, "1883").len(), 1);
        assert!(filter_rows(&rows, Some(Kind::Tcp), "gitlab").is_empty());
    }

    #[test]
    fn bytes_are_human_readable() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KB");
        assert_eq!(human_bytes(3 * 1_048_576), "3.0 MB");
    }

    #[test]
    fn names_suggested_from_targets() {
        // 别名整段可用；user@ 取 host 段；点换成 - 并折叠
        assert_eq!(suggested_name("dev-board", &[]), "dev-board");
        assert_eq!(suggested_name("ops@jump.example.com", &[]), "jump-example-com");
        assert_eq!(suggested_name("192.168.5.88", &[]), "192-168-5-88");
        assert_eq!(suggested_name(" user@X-Board ", &[]), "x-board");
        // 端口与会话路径被裁掉
        assert_eq!(suggested_name("dev-board:2222", &[]), "dev-board");
        assert!(suggested_name("-oProxyCommand=calc", &[]).starts_with("oproxycommand"));
        // 与现有规则重名时加序号
        assert_eq!(suggested_name("web", &["web".into()]), "web-2");
        assert_eq!(suggested_name("web", &["web".into(), "WEB-2".into()]), "web-3");
        // 目标无效时兜底
        assert_eq!(suggested_name("@@", &[]), "ssh");
    }

    #[test]
    fn unique_names_skip_taken() {
        assert_eq!(unique_name("a", &[]), "a");
        assert_eq!(unique_name("a", &["a".into(), "A-2".into()]), "a-3");
    }

    #[test]
    fn candidate_ports_start_after_used() {
        let used = [7001u16, 7002, 8443];
        let ports = next_candidate_ports(&used);
        assert_eq!(ports.len(), 64);
        assert_eq!(ports[0], 8444);
        assert_eq!(ports[1], 8445);
        // 没有已用端口时从 7001 起
        assert_eq!(next_candidate_ports(&[])[0], 7001);
        // 相邻端口不重复
        assert_eq!(next_candidate_ports(&[65535]).len(), 1, "到顶为止");
    }
}
