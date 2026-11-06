//! 规则列表：概览页和串口 / SSH / TCP 页共用的紧凑表格行。
//!
//! 一条规则一行（约 44px）：名称 ｜ 路径 ｜ 状态 ｜ 连接 / 流量 ｜ 开关 + 菜单；
//! 列标题只在列表顶部出现一次。规则失败或 SSH 正在重连时，行下方多出一行原因，
//! 带「重试 / 编辑」按钮。

use windui::prelude::*;

use super::{page_body, row_actions};
use crate::gui::model::{self, Kind, RuleRow, Tone};
use crate::gui::ui::{kind_badge, mono, page_header, panel, status_pill, tone_role};
use crate::gui::Gui;

// 各列宽度：表头与数据行共用。权重列按比例分剩余宽度，固定列给定宽。
const W_NAME: f32 = 1.35;
const W_PATH: f32 = 5.2;
const W_ACTIVITY: f32 = 1.2;
const W_STATUS: i32 = 128;
const W_OPS: i32 = 104;
const ROW_H: i32 = 44;

/// 列表顶部的列标题；`path_title` 随类型变（串口 / SSH / TCP 的路径含义不同）。
pub fn header(path_title: &str) -> Element {
    let cell = |text: &str| Element::label(text.to_string()).fg_role(Role::TextMuted);
    Element::row()
        .width_match()
        .bg_role(Role::SurfaceAlt)
        .padding_xy(16, 8)
        .spacing(12)
        .child(cell("名称").weight(W_NAME))
        .child(cell(path_title).weight(W_PATH))
        .child(cell("状态").width(W_STATUS))
        .child(cell("连接 / 流量").weight(W_ACTIVITY))
        .child(cell("操作").width(W_OPS).text_align(Align::End))
}

/// 一条规则。`show_kind` 为真时名称前面带类型徽章（概览页里要区分类型）。
pub fn rule_row(gui: &Gui, r: RuleRow, show_kind: bool) -> Element {
    let metrics = gui.metric_signals(&r.name);

    let mut name = Element::hstack().weight(W_NAME).cross(Align::Center).spacing(8);
    if show_kind {
        name = name.child(kind_badge(r.kind));
    }
    name = name.child(Element::label(r.name.clone()).font_weight(600).max_lines(1).truncate(Truncate::End));
    if !r.tag.is_empty() {
        name = name.child(Element::badge_intent(r.tag.clone(), Intent::Success).tooltip(r.sub.clone()));
    }

    let main = Element::row()
        .width_match()
        .height(ROW_H)
        .cross(Align::Center)
        .padding_xy(16, 0)
        .spacing(12)
        .child(name)
        .child(
            mono(r.path.clone())
                .max_lines(1)
                // 超长时保留开头的监听地址和结尾的目标，省略中间；完整路径在 tooltip 里
                .truncate(Truncate::Middle)
                .tooltip(r.path.clone())
                .weight(W_PATH),
        )
        .child(
            Element::hstack()
                .width(W_STATUS)
                .child(status_pill(&r.status, r.tone).tooltip(r.detail.clone())),
        )
        .child(
            Element::label(metrics.activity)
                .fg_role(Role::TextMuted)
                .max_lines(1)
                .truncate(Truncate::End)
                .weight(W_ACTIVITY),
        )
        .child(
            Element::hstack()
                .width(W_OPS)
                .child(Element::flex_spacer())
                .child(row_actions(gui, &r)),
        );

    let mut body = Element::col().width_match().child(main);
    if let Some(notice) = model::notice(&r) {
        body = body.child(notice_line(gui, &r, notice));
    }
    body.child(Element::divider())
}

/// 行下方的原因行：红色（失败）或橙色（重连中），右侧是「重试 / 编辑」。
fn notice_line(gui: &Gui, r: &RuleRow, notice: model::Notice) -> Element {
    let tint = if notice.tone == Tone::Bad { Role::Danger } else { Role::Warning };
    let mut line = Element::row()
        .width_match()
        .cross(Align::Center)
        .padding_edges(Insets { left: 28, top: 2, right: 16, bottom: 6 })
        .spacing(8)
        .bg_role_alpha(tint, 0.07)
        .child(
            Element::label(format!("└ {}", notice.text))
                .fg_role(tone_role(notice.tone))
                .max_lines(1)
                .truncate(Truncate::End)
                .tooltip(r.detail.clone())
                .weight(1.0),
        );
    // 串口会自动重新扫描设备，不需要重试
    if r.kind != Kind::Serial {
        let (gui, name) = (gui.clone(), r.name.clone());
        line = line.child(Element::button("重试").small().outline().on_click(move |ctx| {
            match gui.restart_rule(&name) {
                Ok(()) => ctx.toast_ok(format!("已重试 {name}")),
                Err(e) => ctx.toast_err(e),
            }
        }));
    }
    let (gui, kind, name) = (gui.clone(), r.kind, r.name.clone());
    line.child(
        Element::button("编辑").small().outline().neutral().on_click(move |_| {
            gui.editors.open(&gui, kind, Some(name.clone()))
        }),
    )
}

/// 带滚动的规则行列表，数据来自一个行信号（按规则名做 keyed diff，没变的行不重建）。
pub fn rule_list(gui: &Gui, rows: Signal<Vec<RuleRow>>, show_kind: bool) -> Element {
    let g = gui.clone();
    Element::list_signal(rows, |r: &RuleRow| r.name.clone(), move |r: RuleRow| rule_row(&g, r, show_kind))
        .weight(1.0)
}

/// 串口 / SSH / TCP 页：页头 + 一张规则表。
pub fn type_page(gui: &Gui, kind: Kind) -> Element {
    let (index, title, subtitle, add_label, path_title, empty) = match kind {
        Kind::Serial => (
            0,
            "串口共享",
            "把本机串口通过 TCP 共享给局域网内的终端",
            "＋ 添加串口",
            "本机串口 → 终端连接",
            "还没有共享的串口，点右上角「添加串口」。",
        ),
        Kind::Ssh => (
            1,
            "SSH 转发",
            "通过 SSH 服务器建立本地或远程端口转发（调用系统 ssh）",
            "＋ 添加转发",
            "本机监听 → 服务器 → 目标",
            "还没有 SSH 转发，点右上角「添加转发」。系统需要已安装 OpenSSH 客户端。",
        ),
        Kind::Tcp => (
            2,
            "TCP 转发",
            "把本机端口的流量转发到另一个地址",
            "＋ 添加转发",
            "本机监听 → 目标",
            "还没有 TCP 转发，点右上角「添加转发」。",
        ),
    };
    let rows = gui.by_kind[index];
    let add = Element::button(add_label).on_click({
        let gui = gui.clone();
        move |_| gui.editors.open(&gui, kind, None)
    });
    let empty = Element::label(empty)
        .fg_role(Role::TextMuted)
        .width_match()
        .padding(24)
        .visible_when(move || rows.with(|v| v.is_empty()));

    page_body().child(page_header(title, subtitle, vec![add])).child(
        panel(
            Element::col()
                .width_match()
                .height_match()
                .child(header(path_title))
                .child(empty)
                .child(rule_list(gui, rows, false)),
        )
        .weight(1.0),
    )
}
