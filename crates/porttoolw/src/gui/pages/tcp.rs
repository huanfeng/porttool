//! TCP 转发页：表格 + 说明。

use windui::prelude::*;

use super::overview::data_row;
use super::page_body;
use crate::gui::model::{Kind, RuleRow};
use crate::gui::ui::{page_header, panel};
use crate::gui::Gui;

pub fn view(gui: &Gui) -> Element {
    let add = Element::button("＋ 添加转发").on_click({
        let gui = gui.clone();
        move |_| gui.editors.open(&gui, Kind::Tcp, None)
    });
    let by_kind = gui.by_kind[2];
    let g = gui.clone();
    let header = Element::row()
        .width_match()
        .bg_role(Role::SurfaceAlt)
        .padding_xy(16, 8)
        .spacing(12)
        .child(Element::label("名称").fg_role(Role::TextMuted).weight(1.7))
        .child(Element::label("监听 → 目标").fg_role(Role::TextMuted).weight(3.1))
        .child(Element::label("状态").fg_role(Role::TextMuted).weight(1.3))
        .child(Element::label("连接").fg_role(Role::TextMuted).weight(1.5))
        .child(Element::label("操作").fg_role(Role::TextMuted).width(112).text_align(Align::End));
    let rows = Element::list_signal(by_kind, |r: &RuleRow| r.name.clone(), move |r: RuleRow| data_row(&g, r))
        .weight(1.0);
    let empty = Element::label("还没有 TCP 转发，点右上角「添加转发」。")
        .fg_role(Role::TextMuted)
        .width_match()
        .padding(24)
        .visible_when(move || by_kind.with(|v| v.is_empty()));

    page_body()
        .child(page_header("TCP 转发", "把本机端口的流量转发到另一个地址", vec![add]))
        .child(panel(Element::col().width_match().height_match().child(header).child(empty).child(rows)).weight(1.0))
        .child(
            Element::label("监听地址 0.0.0.0 表示局域网内任何设备都能连接，127.0.0.1 仅本机可用。TCP 转发不做认证。")
                .fg_role(Role::TextMuted)
                .width_match(),
        )
}
