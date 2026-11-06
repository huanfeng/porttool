//! 添加 / 编辑对话框（串口、SSH、TCP）和删除确认。
//!
//! 保存时都基于磁盘上的最新配置修改（`Gui::editable_config`），通过 `Config::validate` 校验后
//! 写盘并让服务重新加载。

use std::cell::RefCell;
use std::rc::Rc;

use porttool_core::config::{
    Direction, Encoding, ForwardRule, SerialRule, SshRule, TcpRule, TerminalMode,
};
use porttool_core::devices;
use porttool_core::ssh_config;
use windui::prelude::*;

use super::model::Kind;
use super::model;
use super::{ui, Gui};

const MODES: [&str; 2] = ["telnet", "raw"];
const ENCODINGS: [&str; 2] = ["utf-8", "gbk"];
/// 波特率下拉候选：常见档位，框里也能直接敲别的值。
const BAUD_RATES: [&str; 16] = [
    "300", "600", "1200", "2400", "4800", "9600", "19200", "38400", "57600", "115200", "230400",
    "460800", "921600", "1000000", "1500000", "2000000",
];
const DIALOG_W: i32 = 560;
const FIELD_W: i32 = 360;
const SHORT_W: i32 = 160;

#[derive(Clone)]
pub struct Editors {
    confirm: Confirm,
    serial: SerialEditor,
    ssh: SshEditor,
    tcp: TcpEditor,
}

impl Editors {
    pub fn new() -> Self {
        Self {
            confirm: Confirm::new(),
            serial: SerialEditor::new(),
            ssh: SshEditor::new(),
            tcp: TcpEditor::new(),
        }
    }

    /// 打开某类规则的编辑对话框；`name` 为 `None` 时新增。
    pub fn open(&self, gui: &Gui, kind: Kind, name: Option<String>) {
        let config = gui.editable_config().unwrap_or_else(|_| gui.service.config());
        match kind {
            Kind::Serial => self.serial.open(&config, name),
            Kind::Ssh => self.ssh.open(&config, name),
            Kind::Tcp => self.tcp.open(&config, name),
        }
    }

    pub fn confirm_delete(&self, name: String) {
        self.confirm.open(name);
    }

    pub fn view(&self, gui: &Gui) -> Element {
        Element::stack()
            .child(self.serial.view(gui))
            .child(self.ssh.view(gui))
            .child(self.tcp.view(gui))
            .child(self.confirm.view(gui))
    }
}

/// 把规则写回配置：编辑时替换原名的那条，新增时追加。保存成功返回 `Ok`。
fn store(gui: &Gui, original: Option<&str>, rule: ForwardRule) -> Result<(), String> {
    let mut config = gui.editable_config()?;
    let slot = original.and_then(|o| config.rules.iter().position(|r| r.name() == o));
    match slot {
        Some(i) => config.rules[i] = rule,
        None => config.rules.push(rule),
    }
    config.validate().map_err(|e| format!("{e:#}"))?;
    gui.apply_config(config).map_err(|e| format!("{e:#}"))
}

/// 配置里所有被占用的监听端口：串口的 tcp 端口、SSH 的监听端口、TCP 的监听端口和 HTTP 端口。
fn used_ports(config: &porttool_core::Config) -> Vec<u16> {
    let mut used = vec![config.server.http_port];
    for rule in &config.rules {
        match rule {
            ForwardRule::Serial(r) => used.push(r.tcp_port),
            ForwardRule::Ssh(r) => {
                used.push(r.local_port);
                if let Some(p) = r.ssh_port {
                    used.push(p);
                }
            }
            ForwardRule::Tcp(r) => {
                if let Some(p) = r.listen.rsplit(':').next().and_then(|p| p.parse().ok()) {
                    used.push(p);
                }
            }
        }
    }
    used
}

/// 从候选端口里挑第一个能在 `host` 上绑定成功的；找不到返回 `None`（交回人工填写）。
///
/// `host` 要是规则实际会监听的地址：串口跟随 `server.bind`，SSH 本地转发是 127.0.0.1，
/// TCP 转发是它自己的监听地址——换一个地址探测，结果可能和真正运行时不一样。
fn first_free_port(host: &str, used: &[u16]) -> Option<u16> {
    model::next_candidate_ports(used)
        .into_iter()
        .find(|p| std::net::TcpListener::bind((host, *p)).is_ok())
}

/// 从 `start` 起（最多试 64 个）找第一个既没被配置里的规则用、又能在 `host` 上绑定的端口。
fn first_free_port_from(host: &str, start: u16, used: &[u16]) -> Option<u16> {
    (start..=u16::MAX)
        .take(64)
        .find(|p| !used.contains(p) && std::net::TcpListener::bind((host, *p)).is_ok())
}

fn parse_port(text: &str, what: &str) -> Result<u16, String> {
    match text.trim().parse::<u16>() {
        Ok(p) if p > 0 => Ok(p),
        _ => Err(format!("{what}必须是 1–65535 的整数")),
    }
}

/// 端口是否可用：在规则实际会监听的 `host` 上试绑一下。编辑时端口没改就不检查（正被自己占着）。
/// `host` 为 `None`（端口不在本机，如 SSH 远程转发）时只检查格式。
fn port_hint(host: Option<&str>, text: &str, original: Option<u16>) -> String {
    let Ok(port) = text.trim().parse::<u16>() else {
        return "端口必须是 1–65535 的整数".into();
    };
    if port == 0 {
        return "端口必须是 1–65535 的整数".into();
    }
    let Some(host) = host else {
        return String::new();
    };
    if Some(port) == original {
        return String::new();
    }
    match std::net::TcpListener::bind((host, port)) {
        Ok(_) => "✓ 端口可用".into(),
        Err(_) => "✕ 端口已被占用".into(),
    }
}

/// TCP 规则的监听地址（`IP:端口`）能否使用：格式不对或已被占用时给出提示。
/// 编辑时地址没改就不检查（正被自己占着）。
fn listen_hint(text: &str, original: Option<&str>) -> String {
    let text = text.trim();
    let Ok(addr) = text.parse::<std::net::SocketAddr>() else {
        return "监听地址格式应为 IP:端口，例如 0.0.0.0:8080".into();
    };
    if addr.port() == 0 {
        return "端口必须是 1–65535 的整数".into();
    }
    if original == Some(text) {
        return String::new();
    }
    match std::net::TcpListener::bind(addr) {
        Ok(_) => "✓ 地址可用".into(),
        Err(_) => "✕ 地址已被占用".into(),
    }
}

fn error_label(error: Signal<String>) -> Element {
    Element::label_signal(error)
        .fg_role(Role::Danger)
        .width_match()
        .visible_when(move || error.with(|e| !e.is_empty()))
}

fn footer(show: Signal<bool>, save: impl Fn(&mut windui::core::EventCtx) + 'static) -> Element {
    Element::hstack()
        .width_match()
        .child(Element::flex_spacer())
        .child(Element::button("取消").small().outline().neutral().on_click(move |_| show.set(false)))
        .child(Element::button("保存").small().on_click(save))
}

fn hint_label(hint: Signal<String>) -> Element {
    Element::label_signal(hint)
        .fg_role(Role::TextMuted)
        .visible_when(move || hint.with(|h| !h.is_empty()))
}

// ───────────────────────── 串口 ─────────────────────────

/// 设备下拉框的一项。
#[derive(Clone)]
struct DeviceChoice {
    label: String,
    serial_number: Option<String>,
    com: Option<String>,
}

#[derive(Clone)]
struct SerialEditor {
    show: Signal<bool>,
    title: Signal<String>,
    original: Rc<RefCell<Option<(String, u16)>>>,
    choices: Rc<RefCell<Vec<DeviceChoice>>>,
    device_labels: Signal<Vec<String>>,
    device: Signal<usize>,
    name: Signal<String>,
    baud: Signal<String>,
    tcp_port: Signal<String>,
    port_hint: Signal<String>,
    mode: Signal<usize>,
    encoding: Signal<usize>,
    enabled: Signal<bool>,
    preview: Signal<String>,
    error: Signal<String>,
}

impl SerialEditor {
    fn new() -> Self {
        Self {
            show: signal(false),
            title: signal(String::new()),
            original: Rc::new(RefCell::new(None)),
            choices: Rc::new(RefCell::new(Vec::new())),
            device_labels: signal(Vec::new()),
            device: signal(0),
            name: signal(String::new()),
            baud: signal(String::new()),
            tcp_port: signal(String::new()),
            port_hint: signal(String::new()),
            mode: signal(0),
            encoding: signal(0),
            enabled: signal(true),
            preview: signal(String::new()),
            error: signal(String::new()),
        }
    }

    fn open(&self, config: &porttool_core::Config, name: Option<String>) {
        let existing = name
            .as_deref()
            .and_then(|n| config.serial_rules().find(|p| p.name == n))
            .cloned();

        let devices = devices::list_devices();
        let mut choices: Vec<DeviceChoice> = devices
            .iter()
            .map(|d| DeviceChoice {
                label: [
                    Some(d.com.clone()),
                    d.serial_number.as_ref().map(|sn| format!("序列号 {sn}")),
                    d.description
                        .as_deref()
                        .map(|s| s.trim_end_matches(&format!(" ({})", d.com)).to_string()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("  ·  "),
                serial_number: d.serial_number.clone(),
                com: Some(d.com.clone()),
            })
            .collect();

        let mut selected = 0;
        if let Some(p) = &existing {
            match devices::find_device(p, &devices) {
                Some(d) => {
                    selected = devices.iter().position(|x| x.com == d.com).unwrap_or(0);
                    // 选中的就是当前匹配到的设备：保留原来的匹配规则，原样保存时配置不变、串口不重启
                    choices[selected].serial_number = p.serial_number.clone();
                    choices[selected].com = p.com.clone();
                }
                None => {
                    let rule = match (&p.serial_number, &p.com) {
                        (Some(sn), _) => format!("序列号 {sn}"),
                        (None, Some(com)) => com.clone(),
                        (None, None) => "未配置".into(),
                    };
                    choices.insert(
                        0,
                        DeviceChoice {
                            label: format!("{rule}（当前未接入）"),
                            serial_number: p.serial_number.clone(),
                            com: p.com.clone(),
                        },
                    );
                }
            }
        }

        let next_port = config
            .serial_rules()
            .map(|p| p.tcp_port)
            .max()
            .map_or(7001, |p| p.saturating_add(1));
        // 串口端口从现有串口的下一个号接着排；在实际监听地址（server.bind）上试绑，
        // 并避开配置里别的规则已经用了的端口
        let next_port = first_free_port_from(&config.server.bind, next_port, &used_ports(config)).unwrap_or(next_port);
        let p = existing.clone().unwrap_or(SerialRule {
            name: String::new(),
            serial_number: None,
            com: None,
            baud: 115_200,
            tcp_port: next_port,
            mode: TerminalMode::Telnet,
            encoding: Encoding::Utf8,
            enabled: true,
        });

        self.device_labels.set(choices.iter().map(|c| c.label.clone()).collect());
        *self.choices.borrow_mut() = choices;
        *self.original.borrow_mut() = existing.as_ref().map(|e| (e.name.clone(), e.tcp_port));
        self.title.set(if existing.is_some() { "编辑串口".into() } else { "添加串口".into() });
        self.device.set(selected);
        self.name.set(p.name);
        self.baud.set(p.baud.to_string());
        self.tcp_port.set(p.tcp_port.to_string());
        self.port_hint.set(String::new());
        self.mode.set(matches!(p.mode, TerminalMode::Raw) as usize);
        self.encoding.set(matches!(p.encoding, Encoding::Gbk) as usize);
        self.enabled.set(p.enabled);
        self.error.set(String::new());
        self.show.set(true);
    }

    fn save(&self, gui: &Gui) -> Result<(), String> {
        let name = self.name.get().trim().to_string();
        if name.is_empty() {
            return Err("请填写名称".into());
        }
        let choice = self
            .choices
            .borrow()
            .get(self.device.get())
            .cloned()
            .ok_or("请选择设备（没有可选设备时请先接入设备）")?;
        let baud: u32 = self.baud.get().trim().parse().map_err(|_| "波特率必须是正整数")?;
        let tcp_port = parse_port(&self.tcp_port.get(), "TCP 端口")?;
        let rule = SerialRule {
            name,
            serial_number: choice.serial_number,
            com: choice.com,
            baud,
            tcp_port,
            mode: if self.mode.get() == 1 { TerminalMode::Raw } else { TerminalMode::Telnet },
            encoding: if self.encoding.get() == 1 { Encoding::Gbk } else { Encoding::Utf8 },
            enabled: self.enabled.get(),
        };
        let original = self.original.borrow().as_ref().map(|(n, _)| n.clone());
        store(gui, original.as_deref(), ForwardRule::Serial(rule))
    }

    fn view(&self, gui: &Gui) -> Element {
        let show = self.show;
        let this = self.clone();
        let host = gui.host_for_commands();
        let preview = self.preview;
        let tcp_port = self.tcp_port;
        let mode = self.mode;
        let update_preview = {
            let host = host.clone();
            move || {
                let cmd = match mode.get() {
                    0 => format!("telnet {host} {}", tcp_port.get().trim()),
                    _ => format!("nc {host} {}", tcp_port.get().trim()),
                };
                preview.set(cmd);
            }
        };
        update_preview();
        let original_port = {
            let original = self.original.clone();
            move || original.borrow().as_ref().map(|(_, p)| *p)
        };
        let hint = self.port_hint;
        // 终端端口跟随 server.bind；每次现读，设置页改过监听地址也能跟上
        let bind = {
            let gui = gui.clone();
            move || gui.service.config().server.bind
        };

        let body = Element::col()
            .width_match()
            .spacing(windui::theme::current().metrics.space_sm)
            .child(Element::field("名称", Element::text_input(self.name, "例如 esp32").width(FIELD_W)))
            .child(Element::field(
                "设备",
                Element::dropdown_signal(self.device_labels, self.device).width(FIELD_W),
            ))
            .child(Element::field(
                "波特率",
                Element::combo_box(self.baud, BAUD_RATES.to_vec())
                    .numeric()
                    .select_all_on_pick()
                    .width(SHORT_W),
            ))
            .child(Element::field(
                "终端模式",
                Element::segmented(MODES.to_vec(), self.mode)
                    .width(SHORT_W)
                    .on_segment_change({
                        let up = update_preview.clone();
                        move |_, _| up()
                    }),
            ))
            .child(Element::field(
                "TCP 端口",
                Element::hstack()
                    .child(
                        Element::text_input(self.tcp_port, "7001")
                            .numeric()
                            .width(SHORT_W)
                            .on_commit({
                                let up = update_preview.clone();
                                move |_| {
                                    hint.set(port_hint(Some(&bind()), &tcp_port.get(), original_port()));
                                    up();
                                }
                            }),
                    )
                    .child(hint_label(hint)),
            ))
            .child(Element::field("编码", Element::dropdown(ENCODINGS.to_vec(), self.encoding).width(SHORT_W)))
            .child(Element::field("启用", Element::switch(self.enabled)))
            .child(
                Element::col()
                    .spacing(4)
                    .child(Element::label("连接命令预览").fg_role(Role::TextMuted))
                    .child(
                        Element::row()
                            .width_match()
                            .cross(Align::Center)
                            .bg_role(Role::SurfaceAlt)
                            .corner(8.0)
                            .padding_xy(12, 8)
                            .child(ui::mono(preview))
                            .child(Element::flex_spacer())
                            .child(Element::button("复制").small().outline().neutral().on_click(move |ctx| {
                                ctx.clipboard_set(&preview.get());
                                ctx.toast_ok("已复制");
                            })),
                    ),
            )
            .child(error_label(self.error));

        let gui = gui.clone();
        Element::dialog_panel(
            show,
            self.title,
            DIALOG_W,
            move |_| show.set(false),
            body,
            footer(show, move |ctx| match this.save(&gui) {
                Ok(()) => {
                    this.show.set(false);
                    ctx.toast_ok("已保存");
                }
                Err(e) => this.error.set(e),
            }),
        )
    }
}

// ───────────────────────── SSH ─────────────────────────

#[derive(Clone)]
struct SshEditor {
    show: Signal<bool>,
    title: Signal<String>,
    original: Rc<RefCell<Option<(String, u16)>>>,
    name: Signal<String>,
    target: Signal<String>,
    /// `~/.ssh/config` 里的服务器别名，打开对话框时读取，可刷新
    servers: Signal<Vec<String>>,
    /// 打开对话框时的现有规则名，自动取名要避开
    names: Rc<RefCell<Vec<String>>>,
    /// 上一次自动生成的名字：它还显示在名称框里时才被下一次自动生成覆盖
    last_auto: Rc<RefCell<String>>,
    ssh_port: Signal<String>,
    identity: Signal<String>,
    direction: Signal<usize>,
    local_port: Signal<String>,
    port_hint: Signal<String>,
    remote_host: Signal<String>,
    remote_port: Signal<String>,
    reconnect: Signal<bool>,
    enabled: Signal<bool>,
    preview: Signal<String>,
    error: Signal<String>,
}

impl SshEditor {
    fn new() -> Self {
        Self {
            show: signal(false),
            title: signal(String::new()),
            original: Rc::new(RefCell::new(None)),
            name: signal(String::new()),
            target: signal(String::new()),
            servers: signal(Vec::new()),
            names: Rc::new(RefCell::new(Vec::new())),
            last_auto: Rc::new(RefCell::new(String::new())),
            ssh_port: signal(String::new()),
            identity: signal(String::new()),
            direction: signal(0),
            local_port: signal(String::new()),
            port_hint: signal(String::new()),
            remote_host: signal(String::new()),
            remote_port: signal(String::new()),
            reconnect: signal(true),
            enabled: signal(true),
            preview: signal(String::new()),
            error: signal(String::new()),
        }
    }

    fn open(&self, config: &porttool_core::Config, name: Option<String>) {
        self.servers.set(porttool_core::ssh_config::available_servers());
        let existing = name.as_deref().and_then(|n| {
            config.rules.iter().find_map(|r| match r {
                ForwardRule::Ssh(s) if s.name == n => Some(s.clone()),
                _ => None,
            })
        });
        *self.names.borrow_mut() = config.rules.iter().map(|r| r.name().to_string()).collect();
        *self.last_auto.borrow_mut() = existing.as_ref().map(|e| e.name.clone()).unwrap_or_default();
        let r = existing.clone().unwrap_or(SshRule {
            name: String::new(),
            ssh_target: String::new(),
            ssh_port: None,
            identity_file: None,
            direction: Direction::Local,
            // 监听端口自动分配：接着现有规则用过的最大端口号试绑，避开占用
            local_port: first_free_port("127.0.0.1", &used_ports(config)).unwrap_or(8080),
            remote_host: "127.0.0.1".into(),
            remote_port: 80,
            auto_reconnect: true,
            enabled: true,
        });
        *self.original.borrow_mut() = existing.as_ref().map(|e| (e.name.clone(), e.local_port));
        self.title.set(if existing.is_some() { "编辑 SSH 转发".into() } else { "添加 SSH 转发".into() });
        self.name.set(r.name);
        self.target.set(r.ssh_target);
        self.ssh_port.set(r.ssh_port.map(|p| p.to_string()).unwrap_or_default());
        self.identity.set(r.identity_file.unwrap_or_default());
        self.direction.set(matches!(r.direction, Direction::Remote) as usize);
        self.local_port.set(r.local_port.to_string());
        self.port_hint.set(String::new());
        self.remote_host.set(r.remote_host);
        self.remote_port.set(r.remote_port.to_string());
        self.reconnect.set(r.auto_reconnect);
        self.enabled.set(r.enabled);
        self.error.set(String::new());
        self.update_preview();
        self.show.set(true);
    }

    /// 监听端口的可用性提示：本地转发（-L）在 127.0.0.1 上试绑；
    /// 远程转发（-R）的端口开在服务器上，本机无从检查，只检查格式。
    fn refresh_port_hint(&self) {
        let original = self.original.borrow().as_ref().map(|(_, p)| *p);
        let host = (self.direction.get() != 1).then_some("127.0.0.1");
        self.port_hint.set(port_hint(host, &self.local_port.get(), original));
    }

    fn update_preview(&self) {
        let flag = if self.direction.get() == 1 { "-R" } else { "-L" };
        let port = self.ssh_port.get();
        let port = port.trim();
        self.preview.set(format!(
            "ssh -N {flag} 127.0.0.1:{}:{}:{}{} {}",
            self.local_port.get().trim(),
            self.remote_host.get().trim(),
            self.remote_port.get().trim(),
            if port.is_empty() { String::new() } else { format!(" -p {port}") },
            self.target.get().trim(),
        ));
    }

    fn save(&self, gui: &Gui) -> Result<(), String> {
        let name = self.name.get().trim().to_string();
        if name.is_empty() {
            return Err("请填写名称".into());
        }
        let ssh_port = match self.ssh_port.get().trim() {
            "" => None,
            t => Some(parse_port(t, "SSH 端口")?),
        };
        let ssh_target = self.target.get().trim().to_string();
        // 目标会原样传给 ssh 命令行，拒绝能被当成选项的输入
        ssh_config::valid_target(&ssh_target).map_err(|e| format!("SSH 服务器：{e}"))?;
        let identity = self.identity.get().trim().to_string();
        let rule = SshRule {
            name,
            ssh_target,
            ssh_port,
            identity_file: (!identity.is_empty()).then_some(identity),
            direction: if self.direction.get() == 1 { Direction::Remote } else { Direction::Local },
            local_port: parse_port(&self.local_port.get(), "监听端口")?,
            remote_host: self.remote_host.get().trim().to_string(),
            remote_port: parse_port(&self.remote_port.get(), "目标端口")?,
            auto_reconnect: self.reconnect.get(),
            enabled: self.enabled.get(),
        };
        let original = self.original.borrow().as_ref().map(|(n, _)| n.clone());
        store(gui, original.as_deref(), ForwardRule::Ssh(rule))
    }

    fn view(&self, gui: &Gui) -> Element {
        let show = self.show;
        let this = self.clone();
        let preview = self.preview;
        let hint = self.port_hint;
        let upd = {
            let t = self.clone();
            move |_: &mut windui::core::EventCtx| t.update_preview()
        };
        // 选好 SSH 服务器后自动取名（只在名称为空或仍是上次自动名时替换，不冲掉手改的名字）
        let auto_name = {
            let t = self.clone();
            move |_: &mut windui::core::EventCtx| {
                if self_is_new(&t.original) {
                    let current = t.name.get();
                    let generated = t.last_auto.borrow().clone();
                    if current.trim().is_empty() || current == generated {
                        let name = model::suggested_name(
                            &t.target.get(),
                            t.names.borrow().as_slice(),
                        );
                        t.last_auto.borrow_mut().clone_from(&name);
                        t.name.set(name);
                    }
                }
            }
        };
        fn self_is_new(original: &RefCell<Option<(String, u16)>>) -> bool {
            original.borrow().is_none()
        }

        let body = Element::col()
            .width_match()
            .spacing(windui::theme::current().metrics.space_sm)
            .child(Element::field("名称", Element::text_input(self.name, "例如 gitlab").width(FIELD_W)))
            .child(Element::field(
                "方向",
                Element::segmented(vec!["本地转发 -L", "远程转发 -R"], self.direction)
                    .width(FIELD_W)
                    .on_segment_change({
                        let t = self.clone();
                        move |_, _| {
                            t.update_preview();
                            // 方向变了，端口检查的地址也跟着变；还没填端口时不提示
                            if !t.local_port.get().trim().is_empty() {
                                t.refresh_port_hint();
                            }
                        }
                    }),
            ))
            .child(Element::field(
                "SSH 服务器",
                Element::hstack()
                    .child(
                        Element::combo_box(self.target, Vec::<String>::new())
                            .options_signal(self.servers)
                            .width(FIELD_W - 48)
                            .on_commit({
                                let upd = upd.clone();
                                let auto_name = auto_name.clone();
                                move |ctx| {
                                    upd(ctx);
                                    auto_name(ctx);
                                }
                            }),
                    )
                    .child(
                        Element::icon_button("⟳")
                            .tooltip("重新读取 ~/.ssh/config")
                            .on_click({
                                let servers = self.servers;
                                move |_| servers.set(ssh_config::available_servers())
                            }),
                    ),
            ))
            .child(Element::field(
                "SSH 端口",
                Element::text_input(self.ssh_port, "留空使用 ssh 配置")
                    .numeric()
                    .width(SHORT_W)
                    .on_commit(upd.clone()),
            ))
            .child(Element::field(
                "私钥路径",
                Element::text_input(self.identity, "留空使用系统配置和 ssh-agent").width(FIELD_W),
            ))
            .child(Element::field(
                "监听端口",
                Element::hstack()
                    .child(Element::text_input(self.local_port, "8080").numeric().width(SHORT_W).on_commit({
                        let t = self.clone();
                        move |_| {
                            t.refresh_port_hint();
                            t.update_preview();
                        }
                    }))
                    .child(hint_label(hint)),
            ))
            .child(Element::field(
                "目标地址",
                Element::text_input(self.remote_host, "127.0.0.1").width(FIELD_W).on_commit(upd.clone()),
            ))
            .child(Element::field(
                "目标端口",
                Element::text_input(self.remote_port, "80").numeric().width(SHORT_W).on_commit(upd),
            ))
            .child(Element::field("断线重连", Element::switch(self.reconnect)))
            .child(Element::field("启用", Element::switch(self.enabled)))
            .child(
                Element::col()
                    .spacing(4)
                    .child(Element::label("等价命令").fg_role(Role::TextMuted))
                    .child(
                        Element::row()
                            .width_match()
                            .cross(Align::Center)
                            .bg_role(Role::SurfaceAlt)
                            .corner(8.0)
                            .padding_xy(12, 8)
                            .child(ui::mono(preview).weight(1.0))
                            .child(Element::button("复制").small().outline().neutral().on_click(move |ctx| {
                                ctx.clipboard_set(&preview.get());
                                ctx.toast_ok("已复制");
                            })),
                    ),
            )
            .child(error_label(self.error));

        let gui = gui.clone();
        Element::dialog_panel(
            show,
            self.title,
            DIALOG_W,
            move |_| show.set(false),
            body,
            footer(show, move |ctx| match this.save(&gui) {
                Ok(()) => {
                    this.show.set(false);
                    ctx.toast_ok("已保存");
                }
                Err(e) => this.error.set(e),
            }),
        )
    }
}

// ───────────────────────── TCP ─────────────────────────

#[derive(Clone)]
struct TcpEditor {
    show: Signal<bool>,
    title: Signal<String>,
    original: Rc<RefCell<Option<String>>>,
    /// 编辑时规则原来的监听地址：没改就不检查占用
    original_listen: Rc<RefCell<Option<String>>>,
    name: Signal<String>,
    listen: Signal<String>,
    listen_hint: Signal<String>,
    target: Signal<String>,
    max_conns: Signal<String>,
    enabled: Signal<bool>,
    error: Signal<String>,
}

impl TcpEditor {
    fn new() -> Self {
        Self {
            show: signal(false),
            title: signal(String::new()),
            original: Rc::new(RefCell::new(None)),
            original_listen: Rc::new(RefCell::new(None)),
            name: signal(String::new()),
            listen: signal(String::new()),
            listen_hint: signal(String::new()),
            target: signal(String::new()),
            max_conns: signal(String::new()),
            enabled: signal(true),
            error: signal(String::new()),
        }
    }

    fn open(&self, config: &porttool_core::Config, name: Option<String>) {
        let existing = name.as_deref().and_then(|n| {
            config.rules.iter().find_map(|r| match r {
                ForwardRule::Tcp(t) if t.name == n => Some(t.clone()),
                _ => None,
            })
        });
        let r = existing.clone().unwrap_or_else(|| {
            // 新规则默认监听 0.0.0.0，就在 0.0.0.0 上试绑
            let port = first_free_port("0.0.0.0", &used_ports(config)).unwrap_or(8080);
            TcpRule {
                name: String::new(),
                listen: format!("0.0.0.0:{port}"),
                target: "192.168.1.100:80".into(),
                max_connections: 1000,
                enabled: true,
            }
        });
        *self.original.borrow_mut() = existing.as_ref().map(|e| e.name.clone());
        *self.original_listen.borrow_mut() = existing.as_ref().map(|e| e.listen.clone());
        self.listen_hint.set(String::new());
        self.title.set(if existing.is_some() { "编辑 TCP 转发".into() } else { "添加 TCP 转发".into() });
        self.name.set(r.name);
        self.listen.set(r.listen);
        self.target.set(r.target);
        self.max_conns.set(r.max_connections.to_string());
        self.enabled.set(r.enabled);
        self.error.set(String::new());
        self.show.set(true);
    }

    fn save(&self, gui: &Gui) -> Result<(), String> {
        let name = self.name.get().trim().to_string();
        if name.is_empty() {
            return Err("请填写名称".into());
        }
        let max_connections: usize = self
            .max_conns
            .get()
            .trim()
            .parse()
            .ok()
            .filter(|n| *n > 0)
            .ok_or("最大连接数必须是正整数")?;
        let rule = TcpRule {
            name,
            listen: self.listen.get().trim().to_string(),
            target: self.target.get().trim().to_string(),
            max_connections,
            enabled: self.enabled.get(),
        };
        let original = self.original.borrow().clone();
        store(gui, original.as_deref(), ForwardRule::Tcp(rule))
    }

    fn view(&self, gui: &Gui) -> Element {
        let show = self.show;
        let this = self.clone();
        let body = Element::col()
            .width_match()
            .spacing(windui::theme::current().metrics.space_sm)
            .child(Element::field("名称", Element::text_input(self.name, "例如 web-ui").width(FIELD_W)))
            .child(Element::field(
                "监听地址",
                Element::text_input(self.listen, "0.0.0.0:8080").width(FIELD_W).on_commit({
                    let (listen, hint, original) = (self.listen, self.listen_hint, self.original_listen.clone());
                    move |_| hint.set(listen_hint(&listen.get(), original.borrow().as_deref()))
                }),
            ))
            .child(Element::label("格式为 IP:端口；0.0.0.0 表示局域网可访问，127.0.0.1 仅本机")
                .fg_role(Role::TextMuted))
            .child(hint_label(self.listen_hint))
            .child(Element::field(
                "目标地址",
                Element::text_input(self.target, "192.168.1.100:80").width(FIELD_W),
            ))
            .child(Element::field(
                "最大连接数",
                Element::text_input(self.max_conns, "1000").numeric().width(SHORT_W),
            ))
            .child(Element::field("启用", Element::switch(self.enabled)))
            .child(error_label(self.error));

        let gui = gui.clone();
        Element::dialog_panel(
            show,
            self.title,
            DIALOG_W,
            move |_| show.set(false),
            body,
            footer(show, move |ctx| match this.save(&gui) {
                Ok(()) => {
                    this.show.set(false);
                    ctx.toast_ok("已保存");
                }
                Err(e) => this.error.set(e),
            }),
        )
    }
}

// ───────────────────────── 删除确认 ─────────────────────────

#[derive(Clone)]
struct Confirm {
    show: Signal<bool>,
    target: Signal<String>,
    message: Signal<String>,
}

impl Confirm {
    fn new() -> Self {
        Self {
            show: signal(false),
            target: signal(String::new()),
            message: signal(String::new()),
        }
    }

    fn open(&self, name: String) {
        self.message
            .set(format!("删除规则「{name}」？正在使用的连接会断开，串口日志文件会保留。"));
        self.target.set(name);
        self.show.set(true);
    }

    fn view(&self, gui: &Gui) -> Element {
        let show = self.show;
        let this = self.clone();
        let gui = gui.clone();
        let footer = Element::hstack()
            .width_match()
            .child(Element::flex_spacer())
            .child(Element::button("取消").small().outline().neutral().on_click(move |_| show.set(false)))
            .child(Element::button("删除").small().danger().on_click(move |ctx| {
                let target = this.target.get();
                match gui.delete_rule(&target) {
                    Ok(()) => ctx.toast_ok(format!("已删除 {target}")),
                    Err(e) => ctx.toast_err(e),
                }
                this.show.set(false);
            }));
        Element::dialog_panel(
            show,
            "删除规则",
            420,
            move |_| show.set(false),
            Element::label_signal(self.message).width_match(),
            footer,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use porttool_core::Config;

    #[test]
    fn saving_new_and_edited_rules_goes_through_validation() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, gui) = super::super::tests::setup(dir.path());
        let ed = TcpEditor::new();

        // 新增
        ed.open(&gui.service.config(), None);
        ed.name.set("db".into());
        ed.listen.set("127.0.0.1:0".into());
        ed.target.set("127.0.0.1:9".into());
        ed.max_conns.set("50".into());
        ed.save(&gui).unwrap();
        assert_eq!(Config::load(gui.service.config_path()).unwrap().rules.len(), 2);

        // 编辑：改名后原来那条被替换而不是多出一条
        ed.open(&gui.service.config(), Some("db".into()));
        assert_eq!(ed.max_conns.get(), "50");
        ed.name.set("db2".into());
        ed.save(&gui).unwrap();
        let names: Vec<_> = Config::load(gui.service.config_path())
            .unwrap()
            .rules
            .iter()
            .map(|r| r.name().to_string())
            .collect();
        assert_eq!(names, ["web", "db2"]);

        // 非法输入给出提示、不写盘
        ed.name.set("web".into());
        assert!(ed.save(&gui).unwrap_err().contains("重复"));
        ed.name.set("ok".into());
        ed.listen.set("not-an-address".into());
        assert!(ed.save(&gui).is_err());
        ed.listen.set("127.0.0.1:0".into());
        ed.max_conns.set("0".into());
        assert!(ed.save(&gui).is_err());
        assert_eq!(Config::load(gui.service.config_path()).unwrap().rules.len(), 2);
        runtime.block_on(gui.service.shutdown());
    }

    #[test]
    fn ssh_editor_rejects_option_like_target() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, gui) = super::super::tests::setup(dir.path());
        let ed = SshEditor::new();
        ed.open(&gui.service.config(), None);
        ed.name.set("evil".into());
        ed.target.set("-oProxyCommand=calc".into());
        assert!(ed.save(&gui).is_err());
        ed.target.set("user@host".into());
        ed.local_port.set("0".into());
        assert!(ed.save(&gui).unwrap_err().contains("监听端口"));
        runtime.block_on(gui.service.shutdown());
    }

    #[test]
    fn port_validation() {
        assert_eq!(parse_port(" 8080 ", "端口").unwrap(), 8080);
        assert!(parse_port("0", "端口").is_err());
        assert!(parse_port("70000", "端口").is_err());
        assert!(parse_port("abc", "端口").is_err());
    }

    #[test]
    fn port_hint_detects_occupied_and_unchanged() {
        let l = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let any = Some("0.0.0.0");
        assert!(port_hint(any, &port.to_string(), None).contains("占用"));
        assert!(port_hint(any, &port.to_string(), Some(port)).is_empty(), "未修改的端口不检查");
        drop(l);
        assert!(eventually(|| port_hint(any, &port.to_string(), None).contains("可用")));
        assert!(port_hint(any, "x", None).contains("整数"));
    }

    #[test]
    fn port_probe_uses_the_address_the_rule_will_listen_on() {
        // 127.0.0.1 上的端口被占：按 127.0.0.1 探测（SSH 本地转发、仅本机的串口）必须报占用
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(port_hint(Some("127.0.0.1"), &port.to_string(), None).contains("占用"));
        // 远程转发的端口在服务器上：不探测本机，只检查格式
        assert!(port_hint(None, &port.to_string(), None).is_empty());
        assert!(port_hint(None, "0", None).contains("整数"));

        // 自动分配跳过被占用的端口，并且只在给定地址上探测
        let found = first_free_port("127.0.0.1", &[port - 1]).unwrap();
        assert_ne!(found, port, "被占用的候选应被跳过");
        assert!(found > port - 1);
        drop(l);
        assert!(eventually(|| first_free_port("127.0.0.1", &[port - 1]) == Some(port)), "释放后又可用");
    }

    #[test]
    fn serial_default_port_continues_after_serial_ports_and_avoids_used() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        // 从 port 起：被占的 port 跳过
        assert_ne!(first_free_port_from("127.0.0.1", port, &[]), Some(port));
        // 配置里别的规则（可能没在运行，绑不出占用）用着的端口也要跳过
        drop(l);
        assert!(eventually(|| first_free_port_from("127.0.0.1", port, &[]) == Some(port)));
        assert_ne!(first_free_port_from("127.0.0.1", port, &[port]), Some(port));
    }

    #[test]
    fn listen_hint_checks_format_and_occupation() {
        assert!(listen_hint("not-an-address", None).contains("IP:端口"));
        assert!(listen_hint("127.0.0.1:0", None).contains("整数"));
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        assert!(listen_hint(&addr, None).contains("占用"));
        assert!(listen_hint(&addr, Some(&addr)).is_empty(), "地址没改不检查");
        drop(l);
        assert!(eventually(|| listen_hint(&addr, None).contains("可用")));
    }

    /// 刚释放的端口可能恰好被并行的其它测试占着（它们也在绑临时端口），稍等重试。
    fn eventually(mut check: impl FnMut() -> bool) -> bool {
        (0..25).any(|_| {
            check() || {
                std::thread::sleep(std::time::Duration::from_millis(20));
                false
            }
        })
    }
}
