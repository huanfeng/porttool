//! 概览页：统计卡片 + 告警条 + 规则表格（搜索 / 按类型筛选）。

use windui::prelude::*;

use super::{page_body, row_actions};
use crate::gui::model::{Kind, RuleRow, Stats, Tone};
use crate::gui::ui::{kind_badge, mono, page_header, panel, status_pill, tone_role};
use crate::gui::Gui;

// 表格各列的宽度权重（表头与数据行共用）。
const W_NAME: f32 = 1.7;
const W_PATH: f32 = 3.1;
const W_STATUS: f32 = 1.3;
const W_CONN: f32 = 1.5;
const W_OPS: i32 = 112;

pub fn view(gui: &Gui) -> Element {
    page_body()
        .child(page_header(
            "概览",
            "串口、SSH 与 TCP 转发的运行状态",
            vec![super::new_button(gui, "＋ 新建")],
        ))
        .child(stat_cards(gui.stats))
        .child(alert_bar(gui))
        .child(table(gui))
}

fn stat_cards(stats: Signal<Vec<Stats>>) -> Element {
    let pick = |f: fn(&Stats) -> String| stats.map(move |v| v.first().map(f).unwrap_or_default());
    let note_role =
        |f: fn(&Stats) -> Tone| stats.map(move |v| tone_role(v.first().map(f).unwrap_or(Tone::Off)));
    let card = |title: &str, big: Signal<String>, unit: &str, note: Option<(Signal<String>, Signal<Role>)>| {
        let mut head = Element::hstack()
            .width_match()
            .child(Element::label(title.to_string()))
            .child(Element::flex_spacer());
        if let Some((text, role)) = note {
            head = head.child(Element::label_signal(text).fg_role_signal(role).font_weight(600));
        }
        panel(
            Element::col()
                .width_match()
                .padding(16)
                .spacing(6)
                .child(head)
                .child(
                    Element::hstack()
                        .cross(Align::End)
                        .spacing(6)
                        .child(Element::label_signal(big).font_size(26.0).font_weight(700))
                        .child(Element::label(unit.to_string()).fg_role(Role::TextMuted)),
                ),
        )
        .weight(1.0)
    };
    Element::row()
        .width_match()
        .spacing(14)
        .child(card(
            "串口",
            pick(|s| format!("{}/{}", s.serial.online, s.serial.total)),
            "在线",
            Some((pick(|s| s.serial.note.clone()), note_role(|s| s.serial.tone))),
        ))
        .child(card(
            "SSH 转发",
            pick(|s| format!("{}/{}", s.ssh.online, s.ssh.total)),
            "在线",
            Some((pick(|s| s.ssh.note.clone()), note_role(|s| s.ssh.tone))),
        ))
        .child(card(
            "TCP 转发",
            pick(|s| format!("{}/{}", s.tcp.online, s.tcp.total)),
            "在线",
            Some((pick(|s| s.tcp.note.clone()), note_role(|s| s.tcp.tone))),
        ))
        .child(card("活动连接", pick(|s| s.connections.to_string()), "个终端 / 通道", None))
}

/// 有规则失败时显示的告警条；「查看详情」跳到对应页面。
fn alert_bar(gui: &Gui) -> Element {
    let gui_for_close = gui.clone();
    let alerts = gui.alerts;
    let page = gui.page;
    let text = alerts.map(|a| match a.first() {
        None => String::new(),
        Some(r) => {
            let more = if a.len() > 1 { format!("（另有 {} 条）", a.len() - 1) } else { String::new() };
            let what = r.detail.lines().last().unwrap_or("").trim();
            format!("⚠  {} 转发 {} 失败：{what}{more}", r.kind.label(), r.name)
        }
    });
    Element::row()
        .width_match()
        .cross(Align::Center)
        .bg_role_alpha(Role::Warning, 0.16)
        .border_role(Role::Warning, 1)
        .corner(8.0)
        .padding_xy(16, 10)
        .child(Element::label_signal(text).weight(1.0))
        .child(Element::button("查看详情").small().outline().on_click(move |_| {
            let target = alerts.with(|a| a.first().map(|r| r.kind));
            page.set(match target {
                Some(Kind::Serial) => 1,
                Some(Kind::Ssh) => 2,
                Some(Kind::Tcp) => 3,
                None => 0,
            });
        }))
        .child(
            Element::icon_button("×")
                .tooltip("关闭提示（有新的失败时会再次出现）")
                .on_click(move |_| gui_for_close.dismiss_alerts()),
        )
        .spacing(8)
        .visible_when(move || alerts.with(|a| !a.is_empty()))
}

fn table(gui: &Gui) -> Element {
    let toolbar = Element::row()
        .width_match()
        .cross(Align::Center)
        .padding(12)
        .spacing(12)
        .child(
            Element::text_input(gui.query, "搜索名称、端口或主机")
                .leading_icon('\u{1F50D}')
                .width(300),
        )
        .child(Element::flex_spacer())
        .child(Element::segmented(vec!["全部", "串口", "SSH", "TCP"], gui.filter));

    let header = Element::row()
        .width_match()
        .bg_role(Role::SurfaceAlt)
        .padding_xy(16, 8)
        .spacing(12)
        .child(head_cell("名称", W_NAME))
        .child(head_cell("路径", W_PATH))
        .child(head_cell("状态", W_STATUS))
        .child(head_cell("连接 / 流量", W_CONN))
        .child(Element::label("操作").fg_role(Role::TextMuted).width(W_OPS).text_align(Align::End));

    let g = gui.clone();
    let rows = Element::list_signal(gui.shown, |r: &RuleRow| r.name.clone(), move |r: RuleRow| data_row(&g, r))
        .weight(1.0);
    let shown = gui.shown;
    let empty = Element::label("没有匹配的规则，点右上角「新建」添加。")
        .fg_role(Role::TextMuted)
        .width_match()
        .padding(24)
        .visible_when(move || shown.with(|s| s.is_empty()));

    panel(
        Element::col()
            .width_match()
            .height_match()
            .child(toolbar)
            .child(header)
            .child(empty)
            .child(rows),
    )
    .weight(1.0)
}

fn head_cell(text: &str, weight: f32) -> Element {
    Element::label(text.to_string()).fg_role(Role::TextMuted).weight(weight)
}

/// 一行规则；TCP 页也复用它。
pub fn data_row(gui: &Gui, r: RuleRow) -> Element {
    let metrics = gui.metric_signals(&r.name);
    Element::row()
        .width_match()
        .cross(Align::Center)
        .padding_xy(16, 10)
        .spacing(12)
        .child(
            Element::hstack()
                .weight(W_NAME)
                .child(kind_badge(r.kind))
                .child(Element::label(r.name.clone()).font_weight(600)),
        )
        .child(
            Element::col()
                .weight(W_PATH)
                .spacing(2)
                .child(mono(r.path.clone()))
                .child(Element::label(r.sub.clone()).fg_role(Role::TextMuted).font_size(12.0)),
        )
        .child(
            Element::col()
                .weight(W_STATUS)
                .child(status_pill(&r.status, r.tone).tooltip(r.detail.clone())),
        )
        .child(
            Element::col()
                .weight(W_CONN)
                .spacing(2)
                .child(Element::label(metrics.conns))
                .child(Element::label(metrics.traffic).fg_role(Role::TextMuted).font_size(12.0)),
        )
        .child(
            Element::hstack()
                .width(W_OPS)
                .child(Element::flex_spacer())
                .child(row_actions(gui, &r)),
        )
}
