//! 串口共享页：左侧规则卡片，右侧选中串口的详情。

use windui::prelude::*;

use super::page_body;
use crate::gui::model::{Kind, RuleRow, SerialDetail};
use crate::gui::ui::{mono, page_header, panel, status_pill};
use crate::gui::Gui;

const DETAIL_W: i32 = 340;

pub fn view(gui: &Gui) -> Element {
    let g = gui.clone();
    let reload = Element::button("重新加载配置").outline().neutral().on_click({
        let gui = gui.clone();
        move |ctx| match gui.reload_from_file() {
            Ok(()) => ctx.toast_ok("已重新加载配置"),
            Err(e) => ctx.toast_err(format!("{e:#}")),
        }
    });
    let add = Element::button("＋ 添加串口").on_click({
        let gui = gui.clone();
        move |_| gui.editors.open(&gui, Kind::Serial, None)
    });

    let list = Element::col()
        .weight(1.0)
        .height_match()
        .spacing(12)
        .child(
            Element::list_signal(gui.by_kind[0], |r: &RuleRow| r.name.clone(), move |r: RuleRow| {
                // 行间距靠外边距：list_signal 自身不设子项间距
                card(&g, r).margin_edges(Insets { bottom: 12, ..Insets::default() })
            })
            .weight(1.0),
        )
        .child(add_card(gui));

    let g = gui.clone();
    let detail = Element::col().width(DETAIL_W).height_match().child(Element::host_signal(
        gui.serial_detail,
        move |d: SerialDetail| detail_panel(&g, d),
    ));

    page_body()
        .child(page_header("串口共享", "把本机串口通过 TCP 共享给局域网内的终端", vec![reload, add]))
        .child(Element::row().width_match().weight(1.0).spacing(16).child(list).child(detail))
}

fn card(gui: &Gui, r: RuleRow) -> Element {
    let selected = gui.selected_serial;
    let name = r.name.clone();
    let name2 = r.name.clone();
    let accent_bg = move || {
        if selected.with(|s| *s == name2) {
            windui::theme::current().palette.accent.scale_alpha(0.10)
        } else {
            windui::theme::current().palette.surface
        }
    };
    Element::col()
        .width_match()
        .padding(16)
        .spacing(8)
        .corner(10.0)
        .border_role(Role::Divider, 1)
        .clickable()
        .bg_when(accent_bg)
        .on_click(move |_| selected.set(name.clone()))
        .child(
            Element::hstack()
                .width_match()
                .child(Element::label(r.name.clone()).font_size(16.0).font_weight(700))
                .child(status_pill(&r.status, r.tone).tooltip(r.detail.clone()))
                .child(Element::flex_spacer())
                .child(super::row_actions(gui, &r)),
        )
        .child(mono(r.path.clone()))
        .child(
            Element::label(format!("{} · {} · {}", r.conns, r.traffic, r.sub))
                .fg_role(Role::TextMuted),
        )
}

fn add_card(gui: &Gui) -> Element {
    let gui = gui.clone();
    Element::col()
        .width_match()
        .padding(16)
        .corner(10.0)
        .border_role(Role::Border, 1)
        .cross(Align::Center)
        .clickable()
        .on_click(move |_| gui.editors.open(&gui, Kind::Serial, None))
        .child(Element::label("＋ 添加串口").fg_role(Role::AccentText).font_weight(600))
        .child(Element::label("接入设备后可在对话框里直接选择").fg_role(Role::TextMuted))
}

fn detail_panel(gui: &Gui, d: SerialDetail) -> Element {
    let host = gui.host_for_commands();
    let command = format!("telnet {host} {}", d.tcp_port);
    let edit = {
        let (gui, name) = (gui.clone(), d.name.clone());
        Element::button("编辑").small().outline().neutral().on_click(move |_| {
            gui.editors.open(&gui, Kind::Serial, Some(name.clone()))
        })
    };
    let param = |k: &str, v: String| {
        Element::row()
            .width_match()
            .padding_xy(0, 6)
            .child(Element::label(k.to_string()).fg_role(Role::TextMuted))
            .child(Element::flex_spacer())
            .child(mono(v))
    };
    let section = |t: &str| Element::label(t.to_string()).font_weight(700).padding_xy(0, 4);

    let copy = {
        let command = command.clone();
        Element::icon_button("⧉").tooltip("复制").on_click(move |ctx| {
            ctx.clipboard_set(&command);
            ctx.toast_ok("已复制连接命令");
        })
    };

    panel(
        Element::col()
            .width_match()
            .padding(16)
            .spacing(6)
            .child(
                Element::hstack()
                    .width_match()
                    .child(Element::label(format!("{} 详情", d.name)).font_size(16.0).font_weight(700))
                    .child(Element::flex_spacer())
                    .child(edit),
            )
            .child(section("连接方式"))
            .child(
                Element::row()
                    .width_match()
                    .cross(Align::Center)
                    .bg_role(Role::SurfaceAlt)
                    .corner(8.0)
                    .padding_xy(12, 8)
                    .child(mono(command).weight(1.0))
                    .child(copy),
            )
            .child(section("参数"))
            .child(param("设备", d.device))
            .child(param("波特率", d.baud.to_string()))
            .child(param("终端模式", d.mode.to_string()))
            .child(param("编码", d.encoding.to_string()))
            .child(param("监听范围", d.scope))
            .child(section(&format!("已连接终端 · {}", d.clients)))
            .child(param("接收", d.rx))
            .child(param("发送", d.tx))
            .child(
                Element::label("单个终端的明细与断开功能暂未提供。")
                    .fg_role(Role::TextSubtle)
                    .width_match(),
            ),
    )
}
