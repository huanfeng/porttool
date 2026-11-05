//! SSH 转发页：每条规则一张卡片，展示「本机监听 → SSH 服务器 → 目标」三段流程。

use windui::prelude::*;

use super::{more_button, page_body};
use crate::gui::model::{Kind, RuleRow};
use crate::gui::ui::{info_block, page_header, status_pill};
use crate::gui::Gui;

pub fn view(gui: &Gui) -> Element {
    let g = gui.clone();
    let add = Element::button("＋ 添加转发").on_click({
        let gui = gui.clone();
        move |_| gui.editors.open(&gui, Kind::Ssh, None)
    });
    let by_kind = gui.by_kind[1];
    let list = Element::list_signal(by_kind, |r: &RuleRow| r.name.clone(), move |r: RuleRow| {
        card(&g, r).margin_edges(Insets { left: 0, top: 0, right: 0, bottom: 12 })
    })
    .weight(1.0);
    let empty = Element::label("还没有 SSH 转发，点右上角「添加转发」。系统需要已安装 OpenSSH 客户端。")
        .fg_role(Role::TextMuted)
        .visible_when(move || by_kind.with(|v| v.is_empty()));

    page_body()
        .child(page_header(
            "SSH 转发",
            "通过 SSH 服务器建立本地或远程端口转发（调用系统 ssh）",
            vec![add],
        ))
        .child(empty)
        .child(list)
}

fn card(gui: &Gui, r: RuleRow) -> Element {
    let edit = {
        let (gui, name) = (gui.clone(), r.name.clone());
        Element::icon_button("✎").tooltip("编辑").on_click(move |_| {
            gui.editors.open(&gui, Kind::Ssh, Some(name.clone()))
        })
    };
    let detail = r.detail.lines().last().unwrap_or("").trim().to_string();
    let head = Element::row()
        .width_match()
        .cross(Align::Center)
        .spacing(10)
        .child(Element::label(r.name.clone()).font_size(16.0).font_weight(700))
        .child(Element::badge_intent(r.tag.clone(), Intent::Success))
        .child(status_pill(&r.status, r.tone))
        .child(Element::label(detail).fg_role(Role::TextMuted).weight(1.0).max_lines(1))
        .child(super::row_actions_without_more(gui, &r))
        .child(edit)
        .child(more_button(gui, Kind::Ssh, &r.name));

    let arrow = || Element::label("→").font_size(20.0).fg_role(Role::TextMuted);
    let flow = Element::row()
        .width_match()
        .cross(Align::Center)
        .spacing(12)
        .child(info_block("本机监听", &r.from).weight(1.0))
        .child(arrow())
        .child(info_block("SSH 服务器", &r.via).weight(1.0).bg_role_alpha(Role::Success, 0.14))
        .child(arrow())
        .child(info_block("目标", &r.to).weight(1.0));

    Element::col()
        .width_match()
        .padding(16)
        .spacing(12)
        .bg_role(Role::Surface)
        .border_role(Role::Divider, 1)
        .corner(10.0)
        .child(head)
        .child(flow)
}
