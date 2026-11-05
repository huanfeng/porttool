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

/// 行内开关：信号值与配置里的 `enabled` 不一致时，写回配置。
struct Toggle {
    name: String,
    state: Signal<bool>,
    applied: bool,
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
    last_selected: Rc<RefCell<String>>,
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
    filter_key: Rc<RefCell<(usize, String)>>,
    /// 串口页选中的规则名
    selected_serial: Signal<String>,
    autostart: Signal<bool>,
    /// 配置文件解析失败的错误信息；非空时禁止在界面里保存配置
    load_error: Signal<String>,
    http_url: Signal<String>,
    /// 主题：0 亮色、1 暗色、2 跟随系统
    theme_mode: Signal<usize>,
    theme: Rc<RefCell<Option<ThemeHandle>>>,
    toggles: Rc<RefCell<Vec<Toggle>>>,
    editors: editors::Editors,
}

pub fn run(
    service: Arc<Service>,
    runtime: tokio::runtime::Handle,
    load_error: Option<String>,
    open_window: bool,
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
    app = app.single_instance(APP_ID, {
        let pending_open = pending_open.clone();
        move |_argv| pending_open.set(true)
    });

    let last_tray = RefCell::new((tooltip, online));
    let ticks = Cell::new(0u32);
    app = app.on_interval(Duration::from_millis(250), {
        let gui = gui.clone();
        move |ctx| {
            if pending_open.replace(false) {
                ctx.open_window(gui.main_window());
            }
            gui.poll_inputs();
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
        Gui {
            service,
            runtime,
            rows: signal(Vec::new()),
            shown: signal(Vec::new()),
            by_kind: [signal(Vec::new()), signal(Vec::new()), signal(Vec::new())],
            serial_detail: signal(Vec::new()),
            last_selected: Rc::new(RefCell::new(String::new())),
            stats: signal(Vec::new()),
            alerts: signal(Vec::new()),
            summary: signal(String::new()),
            summary_attention: signal(false),
            nav_counts: [signal("0".into()), signal("0".into()), signal("0".into())],
            page: signal(0),
            filter: signal(0),
            query: signal(String::new()),
            filter_key: Rc::new(RefCell::new((0, String::new()))),
            selected_serial: signal(String::new()),
            autostart: signal(autostart::is_enabled()),
            load_error: signal(load_error.unwrap_or_default()),
            http_url: signal(String::new()),
            theme_mode: signal(2),
            theme: Rc::new(RefCell::new(None)),
            toggles: Rc::new(RefCell::new(Vec::new())),
            editors: editors::Editors::new(),
        }
    }

    /// 读取服务状态刷新所有视图信号；返回（托盘提示文字，是否有规则在线）。
    fn refresh(&self) -> (String, bool) {
        let config = self.service.config();
        let statuses = self.runtime.block_on(self.service.list_rules());
        let snapshots = self.service.snapshots();
        let rows = model::build_rows(&config, &statuses, &snapshots);

        if self.rows.with(|r| *r != rows) {
            self.rows.set(rows.clone());
            self.stats.set(vec![model::stats(&rows)]);
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

    /// 输入类状态（筛选、搜索框、行内开关）没有变化回调，由定时器轮询。
    fn poll_inputs(&self) {
        let key = (self.filter.get(), self.query.get());
        if *self.filter_key.borrow() != key {
            *self.filter_key.borrow_mut() = key;
            self.recompute_shown();
        }
        let selected = self.selected_serial.get();
        if *self.last_selected.borrow() != selected {
            self.recompute_detail();
        }
        self.apply_toggles();
    }

    fn recompute_detail(&self) {
        let selected = self.selected_serial.get();
        *self.last_selected.borrow_mut() = selected.clone();
        let config = self.service.config();
        let detail: Vec<_> = self
            .rows
            .with(|rows| model::serial_detail(&config, rows, &selected))
            .into_iter()
            .collect();
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

    /// 登记一个行内开关，由 `apply_toggles` 负责把变化写回配置。
    fn register_toggle(&self, name: &str, state: Signal<bool>) {
        let mut toggles = self.toggles.borrow_mut();
        toggles.retain(|t| t.state.is_alive() && t.name != name);
        toggles.push(Toggle { name: name.to_string(), state, applied: state.get() });
    }

    fn apply_toggles(&self) {
        let pending: Vec<(String, bool)> = {
            let mut toggles = self.toggles.borrow_mut();
            toggles.retain(|t| t.state.is_alive());
            toggles
                .iter_mut()
                .filter_map(|t| {
                    let want = t.state.try_get()?;
                    (want != t.applied).then(|| {
                        t.applied = want;
                        (t.name.clone(), want)
                    })
                })
                .collect()
        };
        for (name, enabled) in pending {
            if let Err(e) = self.set_enabled(&name, enabled) {
                tracing::warn!("切换规则 {name} 失败: {e}");
                self.refresh();
            }
        }
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
        let server = self.service.config().server;
        let host = match server.bind.as_str() {
            "0.0.0.0" | "::" => local_ip().unwrap_or_else(|| "127.0.0.1".into()),
            other => other.to_string(),
        };
        format!("http://{host}:{}/", server.http_port)
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
        let dir = self.service.config().log_dir(self.service.config_path());
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

    fn tcp(name: &str, listen: &str) -> ForwardRule {
        ForwardRule::Tcp(TcpRule {
            name: name.into(),
            listen: listen.into(),
            target: "127.0.0.1:9".into(),
            max_connections: 10,
            enabled: true,
        })
    }

    fn free_addr() -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().to_string()
    }

    pub(super) fn setup(dir: &std::path::Path) -> (tokio::runtime::Runtime, Gui) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let path = dir.join("config.toml");
        let mut config = Config::default();
        config.server.bind = "127.0.0.1".into();
        config.server.http_port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        config.rules.push(tcp("web", &free_addr()));
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

    #[test]
    fn polled_switch_writes_back_to_config() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, gui) = setup(dir.path());
        gui.refresh();
        let on = signal(true);
        gui.register_toggle("web", on);
        on.set(false);
        gui.poll_inputs();
        assert!(!Config::load(gui.service.config_path()).unwrap().rules[0].enabled());
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
}
