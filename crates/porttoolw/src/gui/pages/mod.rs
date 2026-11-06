//! 各页面。

pub mod overview;
pub mod serial;
pub mod settings;
pub mod ssh;
pub mod tcp;

use windui::prelude::*;

use super::model::{Kind, RuleRow};
use crate::gui::ui::status_pill;
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

/// 三个转发页共用的规则卡片：头部 = 名称（+类型徽章）+ 状态胶囊 + 开关 + …菜单，
/// 中部由各页传入。`select` 不为空时（串口页）卡片可点击选中并高亮。
pub fn rule_card(gui: &Gui, r: &RuleRow, middle: Vec<Element>, select: Option<Signal<String>>) -> Element {
    let sel_bg = select.unwrap_or_else(|| signal(String::new()));
    let bg_name = r.name.clone();
    let bg = move || {
        let theme = windui::theme::current();
        if sel_bg.with(|s| *s == bg_name) {
            theme.palette.accent.scale_alpha(0.10)
        } else {
            theme.palette.surface
        }
    };
    let mut head = vec![
        Element::label(r.name.clone()).font_size(16.0).font_weight(700),
        status_pill(&r.status, r.tone).tooltip(r.detail.clone()),
        Element::flex_spacer(),
    ];
    if !r.tag.is_empty() {
        head.insert(1, Element::badge_intent(r.tag.clone(), Intent::Success));
    }
    let actions = row_actions(gui, r);
    head.push(actions);

    let container = Element::col()
        .width_match()
        .padding(16)
        .spacing(12)
        .border_role(Role::Divider, 1)
        .corner(10.0)
        .bg_when(bg);
    let mut card = match select {
        Some(sel) => {
            let name = r.name.clone();
            container.clickable().on_click(move |_| sel.set(name.clone()))
        }
        None => container,
    };
    card = card
        .child(Element::hstack().width_match().cross(Align::Center).spacing(10).children(head))
        .children(middle);
    card
}

/// 卡片副行：连接数 · 流量 · 参数（串口与 TCP 页共用）。
pub fn sub_line(r: &RuleRow) -> Element {
    Element::label(format!("{} · {} · {}", r.conns, r.traffic, r.sub)).fg_role(Role::TextMuted)
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
