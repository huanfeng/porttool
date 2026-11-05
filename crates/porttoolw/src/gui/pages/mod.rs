//! 各页面。

pub mod overview;
pub mod serial;
pub mod settings;
pub mod ssh;
pub mod tcp;

use windui::prelude::*;

use super::model::{Kind, RuleRow};
use super::Gui;

/// 页面内容的统一外框：24px 内边距的纵向容器。
pub fn page_body() -> Element {
    Element::col().fill().padding(24).spacing(16)
}

/// 行内启用开关。开关的变化由 `Gui::poll_inputs` 写回配置。
pub fn row_actions_without_more(gui: &Gui, row: &RuleRow) -> Element {
    let on = signal(row.enabled);
    gui.register_toggle(&row.name, on);
    Element::switch(on)
}

/// 行末的「开关 + 更多」。
pub fn row_actions(gui: &Gui, row: &RuleRow) -> Element {
    Element::hstack()
        .cross(Align::Center)
        .spacing(8)
        .child(row_actions_without_more(gui, row))
        .child(more_button(gui, row.kind, &row.name))
}

/// 「…」按钮：编辑 / 删除菜单。
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
        let mut req = MenuRequest::below(ctx.bounds(), vec![edit, MenuItem::separator(), delete]);
        req.click_through = false;
        ctx.show_menu_request(req);
    })
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
