//! SSH 转发页：每条规则一张统一风格的规则卡片，展示「本机监听 → SSH 服务器 → 目标」三段流程。

use windui::prelude::*;

use super::{page_body, rule_card};
use crate::gui::model::{Kind, RuleRow};
use crate::gui::ui::{info_block, page_header};
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
    let middle = {
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
        // 重连详情只在有内容时显示（正常/停止时为空）
        let detail = r.detail.lines().last().unwrap_or("").trim().to_string();
        (!detail.is_empty())
            .then(|| {
                Element::label(detail)
                    .fg_role(Role::TextMuted)
                    .max_lines(1)
                    .width_match()
            })
            .into_iter()
            .chain(std::iter::once(flow))
            .collect::<Vec<_>>()
    };
    rule_card(gui, &r, middle, None)
}
