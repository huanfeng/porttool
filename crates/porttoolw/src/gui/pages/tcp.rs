//! TCP 转发页：与串口 / SSH 页统一的规则卡片，「本机监听 → 目标」两段流程。

use windui::prelude::*;

use super::{page_body, rule_card, sub_line};
use crate::gui::model::{Kind, RuleRow};
use crate::gui::ui::{info_block, page_header};
use crate::gui::Gui;

pub fn view(gui: &Gui) -> Element {
    let g = gui.clone();
    let add = Element::button("＋ 添加转发").on_click({
        let gui = gui.clone();
        move |_| gui.editors.open(&gui, Kind::Tcp, None)
    });
    let by_kind = gui.by_kind[2];
    let list = Element::list_signal(by_kind, |r: &RuleRow| r.name.clone(), move |r: RuleRow| {
        card(&g, r).margin_edges(Insets { left: 0, top: 0, right: 0, bottom: 12 })
    })
    .weight(1.0);
    let empty = Element::label("还没有 TCP 转发，点右上角「添加转发」。")
        .fg_role(Role::TextMuted)
        .visible_when(move || by_kind.with(|v| v.is_empty()));

    page_body()
        .child(page_header("TCP 转发", "把本机端口的流量转发到另一个地址", vec![add]))
        .child(empty)
        .child(list)
        .child(
            Element::label(
                "监听地址 0.0.0.0 表示局域网内任何设备都能连接，127.0.0.1 仅本机可用。TCP 转发不做认证。",
            )
            .fg_role(Role::TextMuted)
            .width_match(),
        )
}

fn card(gui: &Gui, r: RuleRow) -> Element {
    let arrow = || Element::label("→").font_size(20.0).fg_role(Role::TextMuted);
    let flow = Element::row()
        .width_match()
        .cross(Align::Center)
        .spacing(12)
        .child(info_block("本机监听", &r.from).weight(1.0))
        .child(arrow())
        .child(info_block("目标", &r.to).weight(1.0));
    rule_card(gui, &r, vec![flow, sub_line(gui, &r)])
}
