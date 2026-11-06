//! 托盘常驻界面（windui 常驻模式）：托盘菜单 + 按需打开的主窗口。
//!
//! 服务运行在后台 tokio 运行时里；UI 线程用定时器读取快照写进信号，不需要跨线程消息。
//! 模块划分：`model` 视图模型（纯函数）、`ui` 通用小部件、`shell` 窗口外壳、`pages` 各页面、
//! `editors` 添加 / 编辑对话框。

mod editors;
pub mod model;
mod pages;
mod shell;
mod ui;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use porttool_core::config::{Config, ForwardRule};
use porttool_core::service::Service;
use windui::prelude::*;

use crate::{autostart, icon};
use model::{Kind, RuleRow, Stats};

/// 单实例标识，`main` 里的提前仲裁与 `App::single_instance` 必须一致。
pub const APP_ID: &str = "porttool";

/// HTTP 地址的缓存：`bind` 为通配地址时要查本机局域网 IP（建一个 UDP 套接字），
/// 不该每 0.5 秒查一次。监听地址 / 端口变了立即重算，其余情况每 30 秒刷新一次（网络切换时跟上）。
#[derive(Default)]
struct UrlCache {
    key: Option<(String, u16)>,
    at: Option<std::time::Instant>,
    url: String,
}

impl UrlCache {
    const TTL: Duration = Duration::from_secs(30);

    fn get(
        &mut self,
        bind: &str,
        port: u16,
        now: std::time::Instant,
        local_ip: impl FnOnce() -> Option<String>,
    ) -> String {
        let fresh = self.key.as_ref().is_some_and(|(b, p)| b == bind && *p == port)
            && self.at.is_some_and(|at| now.duration_since(at) < Self::TTL);
        if !fresh {
            let host = match bind {
                "0.0.0.0" | "::" => local_ip().unwrap_or_else(|| "127.0.0.1".into()),
                other => other.to_string(),
            };
            self.url = format!("http://{host}:{port}/");
            self.key = Some((bind.to_string(), port));
            self.at = Some(now);
        }
        self.url.clone()
    }
}

/// 一条规则的实时数据文案（连接数、流量），行内标签直接绑定这些信号。
/// 刷新时只改这些信号，规则行（`RuleRow`）不变就不会被 `list_signal` 重建。
#[derive(Clone, Copy)]
pub struct MetricSignals {
    /// `2 个连接` / `—`
    pub conns: Signal<String>,
    /// `↓ 27.3 MB ↑ 9.0 KB` / `—`
    pub traffic: Signal<String>,
    /// 串口详情用：接收 / 发送字节数、连接数
    pub rx: Signal<String>,
    pub tx: Signal<String>,
    pub clients: Signal<String>,
}

impl MetricSignals {
    fn new() -> Self {
        let dash = || signal(String::from("—"));
        Self { conns: dash(), traffic: dash(), rx: dash(), tx: dash(), clients: signal(String::from("0")) }
    }

    fn apply(&self, m: &model::Metrics) {
        let set = |sig: Signal<String>, text: String| {
            if sig.with(|t| *t != text) {
                sig.set(text);
            }
        };
        set(self.conns, m.conns_text());
        set(self.traffic, m.traffic_text());
        set(self.rx, m.rx_text());
        set(self.tx, m.tx_text());
        set(self.clients, m.clients_text());
    }
}

/// 一条规则的启用开关。概览页和类型页各有一个开关控件，但都绑定同一个信号，所以两处联动；
/// 用户拨动后由 `Gui::poll_inputs` 通过 `version()` 发现变化并写回配置。
struct SwitchEntry {
    state: Signal<bool>,
    /// 已经处理过的信号版本：版本变了说明有人拨动（或被我们自己对齐）
    seen: u64,
    /// 配置里当前的 enabled（最近一次确认的值）
    last: bool,
}

#[derive(Clone)]
pub struct Gui {
    service: Arc<Service>,
    runtime: tokio::runtime::Handle,
    /// 所有规则的行
    rows: Signal<Vec<RuleRow>>,
    /// 概览表格当前显示的行（按类型和关键字筛选后）
    shown: Signal<Vec<RuleRow>>,
    /// 按类型分开的行（串口 / SSH / TCP 各页用）
    by_kind: [Signal<Vec<RuleRow>>; 3],
    /// 串口页右侧的详情（0 或 1 个元素）
    serial_detail: Signal<Vec<model::SerialDetail>>,
    /// 串口详情已按哪个版本的选中信号计算过
    detail_seen: Rc<Cell<u64>>,
    /// 单元素向量：概览统计卡片据此重建
    stats: Signal<Vec<Stats>>,
    /// 失败的规则：告警条据此显示
    alerts: Signal<Vec<RuleRow>>,
    /// 顶部状态胶囊文字与是否有需处理项
    summary: Signal<String>,
    summary_attention: Signal<bool>,
    /// 侧栏的数量徽章（串口 / SSH / TCP）
    nav_counts: [Signal<String>; 3],
    /// 当前页：0 概览、1 串口、2 SSH、3 TCP、4 设置
    page: Signal<usize>,
    /// 概览筛选：0 全部、1 串口、2 SSH、3 TCP
    filter: Signal<usize>,
    query: Signal<String>,
    /// 概览表格已按哪个版本的（筛选, 搜索）计算过
    filter_seen: Rc<Cell<(u64, u64)>>,
    /// 串口页选中的规则名
    selected_serial: Signal<String>,
    autostart: Signal<bool>,
    /// 配置文件解析失败的错误信息；非空时禁止在界面里保存配置
    load_error: Signal<String>,
    http_url: Signal<String>,
    /// 主题：0 亮色、1 暗色、2 跟随系统
    theme_mode: Signal<usize>,
    theme: Rc<RefCell<Option<ThemeHandle>>>,
    /// 规则名 → 启用开关（跨页面共享）。信号不放进行作用域，行重建后仍然有效
    switches: Rc<RefCell<HashMap<String, SwitchEntry>>>,
    /// 规则名 → 实时数据信号；同样建在行作用域之外
    metrics: Rc<RefCell<HashMap<String, MetricSignals>>>,
    url_cache: Rc<RefCell<UrlCache>>,
    editors: editors::Editors,
}

pub fn run(
    service: Arc<Service>,
    runtime: tokio::runtime::Handle,
    load_error: Option<String>,
    open_window: bool,
    single_instance: bool,
) {
    let gui = Gui::new(service, runtime, load_error);
    let (tooltip, online) = gui.refresh();
    if let Ok(spec) = std::env::var("PORTTOOL_SCREENSHOT_STATE") {
        gui.apply_screenshot_state(&spec);
    }

    let tray = Tray::new()
        .tooltip(tooltip.clone())
        .icon_rgba(32, 32, icon::render(32, Some(online)).rgba())
        .on_left_click({
            let gui = gui.clone();
            move |ctx| ctx.open_window(gui.main_window())
        })
        .menu(tray_menu(&gui));

    let mut app = App::resident("PortTool")
        .icon(IconSource::sized(|size| icon::render(size, None)))
        .tray(tray);
    let tray_handle = app.tray_handle();
    *gui.theme.borrow_mut() = Some(app.theme_handle());
    gui.apply_theme();

    // 第二次启动时打开主窗口。单实例回调拿不到 ctx，记一个标志交给定时器处理。
    let pending_open = Rc::new(Cell::new(false));
    if single_instance {
        app = app.single_instance(APP_ID, {
            let pending_open = pending_open.clone();
            move |_argv| pending_open.set(true)
        });
    }

    let last_tray = RefCell::new((tooltip, online));
    let ticks = Cell::new(0u32);
    app = app.on_interval(Duration::from_millis(250), {
        let gui = gui.clone();
        move |ctx| {
            if pending_open.replace(false) {
                ctx.open_window(gui.main_window());
            }
            for error in gui.poll_inputs() {
                ctx.toast_err(error);
            }
            ticks.set(ticks.get().wrapping_add(1));
            if !ticks.get().is_multiple_of(2) {
                return;
            }
            let (tooltip, online) = gui.refresh();
            let mut last = last_tray.borrow_mut();
            if last.0 != tooltip {
                tray_handle.set_tooltip(tooltip.clone());
            }
            if last.1 != online {
                tray_handle.set_icon_rgba(32, 32, icon::render(32, Some(online)).rgba());
            }
            *last = (tooltip, online);
        }
    });

    if open_window {
        app = app.start_window(gui.main_window());
    }
    app.screenshot_window(gui.main_window())
        .screenshot_from_args()
        .run_resident();
}

fn tray_menu(gui: &Gui) -> Vec<TrayMenuItem> {
    let g = gui.clone();
    let open = move |ctx: &mut TrayCtx| ctx.open_window(g.main_window());
    let g = gui.clone();
    let reload = move |ctx: &mut TrayCtx| match g.reload_from_file() {
        Ok(()) => ctx.notify("PortTool", "已重新加载配置"),
        Err(e) => ctx.notify("重新加载配置失败", &format!("{e:#}")),
    };
    let g = gui.clone();
    let toggle_autostart = move |ctx: &mut TrayCtx| {
        let want = !g.autostart.get();
        match autostart::set_enabled(want) {
            Ok(()) => g.autostart.set(want),
            Err(e) => ctx.notify("设置开机自启失败", &format!("{e:#}")),
        }
    };
    let config_path = gui.service.config_path().to_path_buf();
    let g = gui.clone();
    vec![
        TrayMenuItem::item("打开主窗口", open),
        TrayMenuItem::separator(),
        TrayMenuItem::item("打开日志目录", move |_| g.open_log_dir()),
        TrayMenuItem::item("打开配置文件", move |_| open_config_file(&config_path)),
        TrayMenuItem::item("重新加载配置", reload),
        TrayMenuItem::separator(),
        TrayMenuItem::check("开机自启", gui.autostart, toggle_autostart),
        TrayMenuItem::separator(),
        TrayMenuItem::item("退出", |ctx| ctx.quit()),
    ]
}

impl Gui {
    fn new(service: Arc<Service>, runtime: tokio::runtime::Handle, load_error: Option<String>) -> Self {
        let gui = Gui {
            service,
            runtime,
            rows: signal(Vec::new()),
            shown: signal(Vec::new()),
            by_kind: [signal(Vec::new()), signal(Vec::new()), signal(Vec::new())],
            serial_detail: signal(Vec::new()),
            stats: signal(Vec::new()),
            alerts: signal(Vec::new()),
            summary: signal(String::new()),
            summary_attention: signal(false),
            nav_counts: [signal("0".into()), signal("0".into()), signal("0".into())],
            page: signal(0),
            filter: signal(0),
            query: signal(String::new()),
            filter_seen: Rc::new(Cell::new((0, 0))),
            selected_serial: signal(String::new()),
            autostart: signal(autostart::is_enabled()),
            load_error: signal(load_error.unwrap_or_default()),
            http_url: signal(String::new()),
            theme_mode: signal(2),
            theme: Rc::new(RefCell::new(None)),
            switches: Rc::new(RefCell::new(HashMap::new())),
            metrics: Rc::new(RefCell::new(HashMap::new())),
            url_cache: Rc::new(RefCell::new(UrlCache::default())),
            editors: editors::Editors::new(),
            detail_seen: Rc::new(Cell::new(0)),
        };
        gui.filter_seen.set((gui.filter.version(), gui.query.version()));
        gui.detail_seen.set(gui.selected_serial.version());
        gui
    }

    /// 读取服务状态刷新所有视图信号；返回（托盘提示文字，是否有规则在线）。
    fn refresh(&self) -> (String, bool) {
        let config = self.service.config_arc();
        let statuses = self.runtime.block_on(self.service.list_rules());
        let snapshots = self.service.snapshots();
        let rows = model::build_rows(&config, &statuses);
        let metrics = model::build_metrics(&config, &statuses, &snapshots);

        // 先把开关和实时数据信号建好 / 对齐，行（在 rows 变化后）重建时才能取到
        self.sync_switches(&rows);
        self.sync_metrics(&rows, &metrics);
        let stats = model::stats(&rows, &metrics);
        if self.stats.with(|s| s.first() != Some(&stats)) {
            self.stats.set(vec![stats]);
        }
        if self.rows.with(|r| *r != rows) {
            self.rows.set(rows.clone());
            self.alerts.set(model::failures(&rows).into_iter().cloned().collect());
            for (i, kind) in [Kind::Serial, Kind::Ssh, Kind::Tcp].into_iter().enumerate() {
                let n = rows.iter().filter(|r| r.kind == kind).count();
                self.nav_counts[i].set(n.to_string());
            }
            // 串口页：选中的规则被删掉后回落到第一条
            let first_serial = rows.iter().find(|r| r.kind == Kind::Serial).map(|r| r.name.clone());
            let keep = self.selected_serial.with(|s| rows.iter().any(|r| r.kind == Kind::Serial && &r.name == s));
            if !keep {
                self.selected_serial.set(first_serial.unwrap_or_default());
            }
            for (i, kind) in [Kind::Serial, Kind::Ssh, Kind::Tcp].into_iter().enumerate() {
                let of_kind: Vec<RuleRow> = rows.iter().filter(|r| r.kind == kind).cloned().collect();
                self.by_kind[i].set(of_kind);
            }
            self.recompute_shown();
            self.recompute_detail();
        }

        let (summary, attention) = model::summary(&rows);
        if self.summary.with(|s| *s != summary) {
            self.summary.set(summary.clone());
        }
        if self.summary_attention.get() != attention {
            self.summary_attention.set(attention);
        }
        let url = self.http_url_text();
        if self.http_url.with(|u| *u != url) {
            self.http_url.set(url);
        }

        let online = rows.iter().filter(|r| r.tone == model::Tone::Ok).count();
        (format!("PortTool — {online}/{} 条规则在线", rows.len()), online > 0)
    }

    /// 输入类状态（筛选、搜索框、选中项、行内开关）没有变化回调，由定时器轮询，
    /// 用信号的 `version()` 判断有没有变化。返回需要提示用户的错误（开关写回失败）。
    fn poll_inputs(&self) -> Vec<String> {
        let key = (self.filter.version(), self.query.version());
        if self.filter_seen.get() != key {
            self.filter_seen.set(key);
            self.recompute_shown();
        }
        if self.detail_seen.get() != self.selected_serial.version() {
            self.recompute_detail();
        }
        self.apply_switches()
    }

    fn recompute_detail(&self) {
        self.detail_seen.set(self.selected_serial.version());
        let selected = self.selected_serial.get();
        let config = self.service.config_arc();
        let detail: Vec<_> = model::serial_detail(&config, &selected).into_iter().collect();
        if self.serial_detail.with(|d| *d != detail) {
            self.serial_detail.set(detail);
        }
    }

    fn recompute_shown(&self) {
        let kind = match self.filter.get() {
            1 => Some(Kind::Serial),
            2 => Some(Kind::Ssh),
            3 => Some(Kind::Tcp),
            _ => None,
        };
        let query = self.query.get();
        let shown = self.rows.with(|rows| model::filter_rows(rows, kind, &query));
        if self.shown.with(|s| *s != shown) {
            self.shown.set(shown);
        }
    }

    /// 取规则的实时数据信号；各页面的标签都绑定它。
    pub fn metric_signals(&self, name: &str) -> MetricSignals {
        let mut map = self.metrics.borrow_mut();
        match map.get(name) {
            Some(m) if m.conns.is_alive() => *m,
            _ => {
                let m = MetricSignals::new();
                map.insert(name.to_string(), m);
                m
            }
        }
    }

    /// 把最新的连接数 / 流量写进各规则的信号（没变的不写）。
    fn sync_metrics(&self, rows: &[RuleRow], metrics: &HashMap<String, model::Metrics>) {
        let default = model::Metrics::default();
        for r in rows {
            self.metric_signals(&r.name).apply(metrics.get(&r.name).unwrap_or(&default));
        }
    }

    /// 取规则的启用开关信号；各页面的开关控件都绑定它。
    ///
    /// 信号在 `refresh` 里创建（行作用域之外），这样行重建不会让它失效。
    pub fn rule_switch(&self, name: &str) -> Signal<bool> {
        let mut map = self.switches.borrow_mut();
        match map.get(name) {
            Some(e) if e.state.is_alive() => e.state,
            _ => {
                // 正常情况下 refresh 已经建好；兜底
                let state = signal(true);
                map.insert(name.to_string(), SwitchEntry { state, seen: state.version(), last: true });
                state
            }
        }
    }

    /// 让每条规则都有一个开关信号，并在配置变了（命令行 / 重新加载等）时把信号对齐到配置。
    ///
    /// 已删除规则的信号不回收：删除行会先播离场动画，期间它还在读信号；一个布尔值的开销可以忽略。
    fn sync_switches(&self, rows: &[RuleRow]) {
        let mut map = self.switches.borrow_mut();
        for r in rows {
            match map.get_mut(&r.name) {
                Some(e) if e.state.is_alive() => {
                    if e.last != r.enabled {
                        e.state.set(r.enabled);
                        e.last = r.enabled;
                        e.seen = e.state.version();
                    }
                }
                _ => {
                    let state = signal(r.enabled);
                    map.insert(r.name.clone(), SwitchEntry { state, seen: state.version(), last: r.enabled });
                }
            }
        }
    }

    /// 把被拨动的开关写回配置。写回失败时开关还原，并返回错误文字供界面提示。
    fn apply_switches(&self) -> Vec<String> {
        let pending: Vec<(String, bool)> = {
            let mut map = self.switches.borrow_mut();
            map.iter_mut()
                .filter_map(|(name, e)| {
                    if !e.state.is_alive() {
                        return None;
                    }
                    let version = e.state.version();
                    if version == e.seen {
                        return None;
                    }
                    e.seen = version;
                    let want = e.state.try_get()?;
                    (want != e.last).then(|| (name.clone(), want))
                })
                .collect()
        };
        let mut errors = Vec::new();
        for (name, want) in pending {
            match self.set_enabled(&name, want) {
                Ok(()) => {
                    if let Some(e) = self.switches.borrow_mut().get_mut(&name) {
                        e.last = want;
                        e.seen = e.state.version();
                    }
                }
                Err(err) => {
                    tracing::warn!("切换规则 {name} 失败: {err}");
                    if let Some(e) = self.switches.borrow_mut().get_mut(&name) {
                        e.state.set(e.last);
                        e.seen = e.state.version();
                    }
                    errors.push(format!("{name}：{err}"));
                }
            }
        }
        errors
    }

    /// 启用 / 停用一条规则并保存配置。
    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), String> {
        let mut config = self.editable_config()?;
        let rule = config
            .rules
            .iter_mut()
            .find(|r| r.name() == name)
            .ok_or_else(|| format!("规则 {name} 不存在"))?;
        match rule {
            ForwardRule::Serial(r) => r.enabled = enabled,
            ForwardRule::Ssh(r) => r.enabled = enabled,
            ForwardRule::Tcp(r) => r.enabled = enabled,
        }
        self.apply_config(config).map_err(|e| format!("{e:#}"))
    }

    pub fn delete_rule(&self, name: &str) -> Result<(), String> {
        let mut config = self.editable_config()?;
        config.rules.retain(|r| r.name() != name);
        self.apply_config(config).map_err(|e| format!("{e:#}"))
    }

    fn http_url_text(&self) -> String {
        let config = self.service.config_arc();
        let server = &config.server;
        self.url_cache
            .borrow_mut()
            .get(&server.bind, server.http_port, std::time::Instant::now(), local_ip)
    }

    /// 本机地址：连接命令预览用。
    fn host_for_commands(&self) -> String {
        let url = self.http_url_text();
        url.trim_start_matches("http://")
            .split(':')
            .next()
            .unwrap_or("127.0.0.1")
            .to_string()
    }

    /// 界面修改配置的基准：重新读取配置文件，而不是用内存里的配置，
    /// 这样用户在文件里的手工修改（还没点重新加载）不会被界面保存冲掉。
    ///
    /// 配置文件解析失败时拒绝：此时内存里是空配置，保存会覆盖用户的原文件。
    fn editable_config(&self) -> Result<Config, String> {
        const HINT: &str = "配置文件有错误，请先修正后点「重新加载配置」（详情见主窗口顶部）";
        if !self.load_error.with(|e| e.is_empty()) {
            return Err(HINT.into());
        }
        Config::load(self.service.config_path()).map_err(|e| {
            self.load_error.set(format!("{e:#}"));
            HINT.to_string()
        })
    }

    /// 保存配置文件并让服务应用新配置。
    fn apply_config(&self, config: Config) -> Result<()> {
        config.save(self.service.config_path())?;
        self.runtime.block_on(self.service.reload(config))?;
        self.refresh();
        Ok(())
    }

    fn reload_from_file(&self) -> Result<()> {
        let config = Config::load(self.service.config_path()).inspect_err(|e| {
            self.load_error.set(format!("{e:#}"));
        })?;
        self.runtime.block_on(self.service.reload(config))?;
        self.load_error.set(String::new());
        self.refresh();
        Ok(())
    }

    fn open_log_dir(&self) {
        let dir = self.service.config_arc().log_dir(self.service.config_path());
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::process::Command::new("explorer").arg(&dir).spawn();
    }

    fn open_config(&self) {
        open_config_file(self.service.config_path());
    }

    /// 按 `theme_mode` 设置亮暗主题。
    fn apply_theme(&self) {
        let dark = match self.theme_mode.get() {
            0 => false,
            1 => true,
            _ => windui::event::system_prefers_dark(),
        };
        if let Some(handle) = self.theme.borrow().as_ref() {
            handle.update(|t| t.palette = shell::palette(dark));
        }
    }

    /// 截图验证用：`PORTTOOL_SCREENSHOT_STATE="page=2;theme=1;dialog=ssh"` 让窗口以指定的页面、
    /// 主题（0 亮 / 1 暗）和对话框（serial / ssh / tcp / delete）打开，便于逐页渲染比对。
    fn apply_screenshot_state(&self, spec: &str) {
        for part in spec.split(';') {
            let Some((k, v)) = part.split_once('=') else { continue };
            match k.trim() {
                "page" => self.page.set(v.trim().parse().unwrap_or(0)),
                "theme" => self.theme_mode.set(v.trim().parse().unwrap_or(0)),
                "dialog" => {
                    let config = self.service.config();
                    let first = |kind: Kind| {
                        config
                            .rules
                            .iter()
                            .find(|r| {
                                matches!(
                                    (kind, r),
                                    (Kind::Serial, ForwardRule::Serial(_))
                                        | (Kind::Ssh, ForwardRule::Ssh(_))
                                        | (Kind::Tcp, ForwardRule::Tcp(_))
                                )
                            })
                            .map(|r| r.name().to_string())
                    };
                    match v.trim() {
                        "serial" => self.editors.open(self, Kind::Serial, None),
                        "serial-edit" => self.editors.open(self, Kind::Serial, first(Kind::Serial)),
                        "ssh" => self.editors.open(self, Kind::Ssh, None),
                        "ssh-edit" => self.editors.open(self, Kind::Ssh, first(Kind::Ssh)),
                        "tcp" => self.editors.open(self, Kind::Tcp, None),
                        "delete" => self.editors.confirm_delete(first(Kind::Tcp).unwrap_or_default()),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        self.apply_theme();
    }

    fn main_window(&self) -> WindowRequest {
        let gui = self.clone();
        Window::new("PortTool", 1100, 740)
            .single("main")
            .centered(true)
            .min_size(900, 560)
            .icon(IconSource::sized(|size| icon::render(size, None)))
            .content(move || shell::main_view(&gui))
    }
}

/// 本机的局域网 IP：向外部地址"连接"一个 UDP 套接字（不发包），读它选中的本地地址。
fn local_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("10.255.255.255:1").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

/// 用默认程序打开配置文件；文件还不存在时先写一份默认配置。
fn open_config_file(path: &Path) {
    if !path.exists() {
        let _ = Config::default().save(path);
    }
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use porttool_core::config::TcpRule;

    fn tcp(name: &str, listen: &str, target: &str) -> ForwardRule {
        ForwardRule::Tcp(TcpRule {
            name: name.into(),
            listen: listen.into(),
            target: target.into(),
            max_connections: 10,
            enabled: true,
        })
    }

    fn free_addr() -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().to_string()
    }

    pub(super) fn setup(dir: &std::path::Path) -> (tokio::runtime::Runtime, Gui) {
        setup_target(dir, "127.0.0.1:9")
    }

    fn setup_target(dir: &std::path::Path, target: &str) -> (tokio::runtime::Runtime, Gui) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let path = dir.join("config.toml");
        let mut config = Config::default();
        config.server.bind = "127.0.0.1".into();
        config.server.http_port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        config.rules.push(tcp("web", &free_addr(), target));
        config.save(&path).unwrap();
        let service = runtime.block_on(Service::start(config, path, |_| {})).unwrap();
        let gui = Gui::new(service, runtime.handle().clone(), None);
        (runtime, gui)
    }

    #[test]
    fn toggle_and_delete_update_config_and_rows() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, gui) = setup(dir.path());
        gui.refresh();
        assert_eq!(gui.rows.with(|r| r.len()), 1);
        assert_eq!(gui.rows.with(|r| r[0].status.clone()), "监听中");

        gui.set_enabled("web", false).unwrap();
        assert_eq!(gui.rows.with(|r| r[0].status.clone()), "已停用");
        assert!(!Config::load(gui.service.config_path()).unwrap().rules[0].enabled());

        gui.set_enabled("web", true).unwrap();
        assert_eq!(gui.rows.with(|r| r[0].status.clone()), "监听中");
        assert!(gui.set_enabled("nope", true).is_err());

        gui.delete_rule("web").unwrap();
        assert!(gui.rows.with(|r| r.is_empty()));
        runtime.block_on(gui.service.shutdown());
    }

    fn web_enabled(gui: &Gui) -> bool {
        Config::load(gui.service.config_path()).unwrap().rules[0].enabled()
    }

    #[test]
    fn switch_toggled_on_any_page_writes_back_and_stays_in_sync() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, gui) = setup(dir.path());
        gui.refresh();
        // 概览页和类型页各为同一规则取一个开关：必须是同一个信号，联动并且都能写回
        let overview = gui.rule_switch("web");
        let kind_page = gui.rule_switch("web");
        assert!(overview.get() && kind_page.get());

        overview.set(false);
        assert!(gui.poll_inputs().is_empty());
        assert!(!web_enabled(&gui), "概览页的开关应写回配置");
        assert!(!kind_page.get(), "另一页的开关应联动");

        kind_page.set(true);
        assert!(gui.poll_inputs().is_empty());
        assert!(web_enabled(&gui), "类型页的开关也应写回配置");
        assert!(overview.get());
        runtime.block_on(gui.service.shutdown());
    }

    #[test]
    fn switch_rolls_back_and_reports_when_save_fails() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, gui) = setup(dir.path());
        gui.refresh();
        gui.load_error.set("解析失败".into()); // 配置有误：禁止保存
        let sw = gui.rule_switch("web");

        sw.set(false);
        let errors = gui.poll_inputs();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("web"), "{errors:?}");
        assert!(sw.get(), "写回失败时开关应还原");
        assert!(web_enabled(&gui), "配置不应被改动");
        assert!(gui.poll_inputs().is_empty(), "还原不应被当成新的拨动而重复报错");
        runtime.block_on(gui.service.shutdown());
    }

    #[test]
    fn switch_follows_external_changes_without_writing_back() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, gui) = setup(dir.path());
        gui.refresh();
        let sw = gui.rule_switch("web");

        // 命令行 / HTTP 停用了这条规则：开关跟着变，而且不会又被当成用户拨动写回去
        let service = gui.service.clone();
        runtime.block_on(service.set_enabled("web", false)).unwrap();
        gui.refresh();
        assert!(!sw.get());
        assert!(gui.poll_inputs().is_empty());
        assert!(!web_enabled(&gui));
        runtime.block_on(gui.service.shutdown());
    }

    #[test]
    fn broken_config_blocks_saving() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, gui) = setup(dir.path());
        gui.load_error.set("解析失败".into());
        assert!(gui.set_enabled("web", false).is_err(), "配置有误时不能保存");
        assert!(gui.delete_rule("web").is_err());
        runtime.block_on(gui.service.shutdown());
    }

    #[test]
    fn filter_and_selection_follow_rows() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, gui) = setup(dir.path());
        gui.refresh();
        gui.filter.set(1); // 只看串口
        gui.poll_inputs();
        assert!(gui.shown.with(|s| s.is_empty()));
        gui.filter.set(3);
        gui.query.set("WEB".into());
        gui.poll_inputs();
        assert_eq!(gui.shown.with(|s| s.len()), 1);
        runtime.block_on(gui.service.shutdown());
    }

    #[test]
    fn metrics_change_does_not_rebuild_rows_but_status_change_does() {
        // 目标是一个只接受连接、不回数据的服务，这样转发连接会一直保持
        let target = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let target_addr = target.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            let mut keep = Vec::new();
            for stream in target.incoming() {
                match stream {
                    Ok(s) => keep.push(s),
                    Err(_) => break,
                }
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let (runtime, gui) = setup_target(dir.path(), &target_addr);
        gui.refresh();
        let conns = gui.metric_signals("web").conns;
        assert_eq!(conns.get(), "0 个连接");
        let rows_version = gui.rows.version();

        // 连进转发端口：连接数变了，但规则行的结构性数据没变
        let listen = match &gui.service.config().rules[0] {
            ForwardRule::Tcp(r) => r.listen.clone(),
            _ => unreachable!(),
        };
        let _client = std::net::TcpStream::connect(&listen).unwrap();
        let mut seen = false;
        for _ in 0..40 {
            gui.refresh();
            if conns.get() == "1 个连接" {
                seen = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(seen, "连接数应更新到信号：{}", conns.get());
        assert_eq!(gui.rows.version(), rows_version, "只有连接数 / 流量变化时不应重建规则行");

        // 状态变化（停用）才会让行变化
        runtime.block_on(gui.service.clone().set_enabled("web", false)).unwrap();
        gui.refresh();
        assert_ne!(gui.rows.version(), rows_version, "状态变化应更新规则行");
        assert_eq!(gui.rows.with(|r| r[0].status.clone()), "已停用");
        assert_eq!(conns.get(), "—", "停用后没有连接数");
        runtime.block_on(gui.service.shutdown());
    }

    #[test]
    fn http_url_is_cached_until_address_changes_or_ttl_expires() {
        use std::cell::Cell;
        use std::time::Instant;
        let calls = Cell::new(0);
        let ip = || {
            calls.set(calls.get() + 1);
            Some("192.168.5.30".to_string())
        };
        let t0 = Instant::now();
        let mut cache = UrlCache::default();

        assert_eq!(cache.get("0.0.0.0", 7000, t0, ip), "http://192.168.5.30:7000/");
        assert_eq!(cache.get("0.0.0.0", 7000, t0 + Duration::from_secs(10), ip), "http://192.168.5.30:7000/");
        assert_eq!(calls.get(), 1, "TTL 内不应重复查本机 IP");

        cache.get("0.0.0.0", 7000, t0 + Duration::from_secs(31), ip);
        assert_eq!(calls.get(), 2, "超过 TTL 要重新查（网络可能变了）");

        // 端口或监听地址变了：立即重算，不等 TTL
        assert_eq!(cache.get("0.0.0.0", 7100, t0 + Duration::from_secs(32), ip), "http://192.168.5.30:7100/");
        assert_eq!(calls.get(), 3);
        assert_eq!(cache.get("127.0.0.1", 7100, t0 + Duration::from_secs(33), ip), "http://127.0.0.1:7100/");
        assert_eq!(calls.get(), 3, "具体地址不需要查本机 IP");

        // 查不到本机 IP 时退回 127.0.0.1
        let mut cache = UrlCache::default();
        assert_eq!(cache.get("::", 7000, t0, || None), "http://127.0.0.1:7000/");
    }

    #[test]
    fn refresh_does_not_clone_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, gui) = setup(dir.path());
        let before = gui.service.config_arc();
        gui.refresh();
        gui.refresh();
        assert!(Arc::ptr_eq(&before, &gui.service.config_arc()), "刷新不应改动配置");
        runtime.block_on(gui.service.shutdown());
    }
}
