//! 串口共享页：与 SSH / TCP 页统一的规则卡片，「本机串口 → 终端连接」两段流程。

use windui::prelude::*;

use super::{page_body, rule_card, sub_line};
use crate::gui::model::{Kind, RuleRow};
use crate::gui::ui::{info_block, page_header};
use crate::gui::Gui;

pub fn view(gui: &Gui) -> Element {
    let g = gui.clone();
    let add = Element::button("＋ 添加串口").on_click({
        let gui = gui.clone();
        move |_| gui.editors.open(&gui, Kind::Serial, None)
    });
    let by_kind = gui.by_kind[0];
    let list = Element::list_signal(by_kind, |r: &RuleRow| r.name.clone(), move |r: RuleRow| {
        card(&g, r).margin_edges(Insets { left: 0, top: 0, right: 0, bottom: 12 })
    })
    .weight(1.0);
    let empty = Element::label("还没有共享的串口，点右上角「添加串口」。")
        .fg_role(Role::TextMuted)
        .visible_when(move || by_kind.with(|v| v.is_empty()));

    page_body()
        .child(page_header("串口共享", "把本机串口通过 TCP 共享给局域网内的终端", vec![add]))
        .child(empty)
        .child(list)
}

fn card(gui: &Gui, r: RuleRow) -> Element {
    let arrow = || Element::label("→").font_size(20.0).fg_role(Role::TextMuted);
    // 终端连接块：点一下复制连接命令（主机在点击时取，监听地址改了也跟着变）
    let connect = {
        let gui = gui.clone();
        let (mode, port) = (r.via.clone(), r.port);
        info_block("终端连接", &r.to)
            .weight(1.0)
            .clickable()
            .tooltip("点击复制连接命令")
            .on_click(move |ctx| {
                let host = gui.host_for_commands();
                let command = if mode == "raw" { format!("nc {host} {port}") } else { format!("telnet {host} {port}") };
                ctx.clipboard_set(&command);
                ctx.toast_ok(format!("已复制：{command}"));
            })
    };
    let flow = Element::row()
        .width_match()
        .cross(Align::Center)
        .spacing(12)
        .child(info_block("本机串口", &r.from).weight(1.0))
        .child(arrow())
        .child(connect);
    rule_card(gui, &r, vec![flow, sub_line(gui, &r)])
}
