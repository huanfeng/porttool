//! 各页面。

pub mod list;
pub mod overview;
pub mod settings;

use windui::prelude::*;

use porttool_core::config::{ForwardRule, TerminalMode};

use super::model::{self, Kind, RuleRow};
use super::Gui;

/// 页面内容的统一外框：24px 内边距的纵向容器。
pub fn page_body() -> Element {
    Element::col().fill().padding(24).spacing(16)
}

/// 行内启用开关：绑定规则共享的开关信号（各页面联动），变化由 `Gui::poll_inputs` 写回配置。
pub fn row_actions_without_more(gui: &Gui, row: &RuleRow) -> Element {
    Element::switch(gui.rule_switch(&row.name))
}

/// 行末的「开关 + 更多」。
pub fn row_actions(gui: &Gui, row: &RuleRow) -> Element {
    Element::hstack()
        .cross(Align::Center)
        .spacing(8)
        .child(row_actions_without_more(gui, row))
        .child(more_button(gui, row.kind, &row.name))
}

/// 「…」按钮：编辑 / 复制命令 / 删除菜单。
pub fn more_button(gui: &Gui, kind: Kind, name: &str) -> Element {
    let gui = gui.clone();
    let name = name.to_string();
    Element::icon_button("…").tooltip("更多").on_click(move |ctx| {
        let edit = {
            let (gui, name) = (gui.clone(), name.clone());
            MenuItem::run("编辑…", move |_| gui.editors.open(&gui, kind, Some(name.clone())), false)
        };
        let delete = {
            let (gui, name) = (gui.clone(), name.clone());
            MenuItem::run("删除", move |_| gui.editors.confirm_delete(name.clone()), false)
        };
        let mut items = vec![edit];
        items.extend(copy_item(&gui, kind, &name));
        items.push(MenuItem::separator());
        items.push(delete);
        let mut req = MenuRequest::below(ctx.bounds(), items);
        req.click_through = false;
        ctx.show_menu_request(req);
    })
}

/// 菜单里「复制命令」要复制什么。
enum CopyTarget {
    /// 串口：终端连接命令，主机在点击时取
    Serial(TerminalMode, u16),
    /// SSH：等价的 ssh 命令行
    Ssh(String),
}

/// 串口规则的「复制连接命令」、SSH 规则的「复制 ssh 命令」。
/// 规则在打开菜单时现取，主机在点击时现取，配置改了也是最新的。
fn copy_item(gui: &Gui, kind: Kind, name: &str) -> Option<MenuItem> {
    let config = gui.service.config_arc();
    let (label, target) = match kind {
        Kind::Serial => {
            let rule = config.serial_rules().find(|r| r.name == name)?;
            ("复制连接命令", CopyTarget::Serial(rule.mode, rule.tcp_port))
        }
        Kind::Ssh => {
            let rule = config.rules.iter().find_map(|r| match r {
                ForwardRule::Ssh(s) if s.name == name => Some(s),
                _ => None,
            })?;
            ("复制 ssh 命令", CopyTarget::Ssh(model::ssh_rule_command(rule)))
        }
        Kind::Tcp => return None,
    };
    let gui = gui.clone();
    Some(MenuItem::run(
        label,
        move |ctx| {
            let text = match &target {
                CopyTarget::Serial(mode, port) => model::connect_command(*mode, &gui.host_for_commands(), *port),
                CopyTarget::Ssh(command) => command.clone(),
            };
            ctx.clipboard_set(&text);
            ctx.toast_ok(format!("已复制：{text}"));
        },
        false,
    ))
}

/// 「新建」按钮：弹出类型菜单。
pub fn new_button(gui: &Gui, label: &str) -> Element {
    let gui = gui.clone();
    Element::button(label.to_string()).on_click(move |ctx| {
        let item = |text: &str, kind: Kind| {
            let gui = gui.clone();
            MenuItem::run(text.to_string(), move |_| gui.editors.open(&gui, kind, None), false)
        };
        let mut req = MenuRequest::below(
            ctx.bounds(),
            vec![item("串口共享", Kind::Serial), item("SSH 转发", Kind::Ssh), item("TCP 转发", Kind::Tcp)],
        );
        req.click_through = false;
        ctx.show_menu_request(req);
    })
}
