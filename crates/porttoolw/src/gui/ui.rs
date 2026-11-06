//! 界面小部件：类型徽章、状态胶囊、页头、等宽文字等，所有颜色走 `Role`，随亮暗主题切换。

use windui::prelude::*;

use super::model::{Kind, Tone};

/// 等宽字体（路径、端口、命令）。
pub const MONO: &str = "Consolas";

pub fn mono(text: impl Into<TextContent>) -> Element {
    Element::label(text).font_family(MONO)
}

/// 规则类型徽章：串口蓝、SSH 绿、TCP 紫。
pub fn kind_badge(kind: Kind) -> Element {
    let intent = match kind {
        Kind::Serial => Intent::Primary,
        Kind::Ssh => Intent::Success,
        Kind::Tcp => Intent::Custom(Color::hex(0x7C5CFC)),
    };
    Element::badge_intent(kind.label(), intent)
}

/// 状态胶囊：`● 已连接`。
pub fn status_pill(text: &str, tone: Tone) -> Element {
    let intent = match tone {
        Tone::Ok => Intent::Success,
        Tone::Warn => Intent::Warning,
        Tone::Bad => Intent::Danger,
        Tone::Off => Intent::Neutral,
    };
    Element::badge_intent(format!("● {text}"), intent)
}

pub fn tone_role(tone: Tone) -> Role {
    match tone {
        Tone::Ok => Role::Success,
        Tone::Warn => Role::Warning,
        Tone::Bad => Role::Danger,
        Tone::Off => Role::TextMuted,
    }
}

/// 页头：左侧标题 + 副标题，右侧操作按钮。
pub fn page_header(title: &str, subtitle: &str, actions: Vec<Element>) -> Element {
    let metrics = windui::theme::current().metrics.clone();
    let left = Element::col()
        .spacing(2)
        .child(Element::label(title).font_size(metrics.font_xl).font_weight(700))
        .child(Element::label(subtitle).fg_role(Role::TextMuted));
    Element::hstack()
        .width_match()
        .child(left)
        .child(Element::flex_spacer())
        .children(actions)
}

/// 圆角卡片容器（无标题）。
pub fn panel(body: Element) -> Element {
    Element::col()
        .width_match()
        .bg_role(Role::Surface)
        .border_role(Role::Divider, 1)
        .corner(10.0)
        .child(body)
}
