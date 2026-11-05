//! 窗口外壳：侧栏导航 + 页面区 + 底部状态栏 + 对话框层。

use windui::prelude::*;

use super::{pages, ui, Gui};

/// 亮暗色板：沿用 windui 默认色板，只把强调色换成设计稿的蓝。
pub fn palette(dark: bool) -> Palette {
    let mut p = if dark { Palette::dark() } else { Palette::default() };
    if dark {
        p.accent = Color::hex(0x3B82F6);
        p.accent_hover = Color::hex(0x60A5FA);
        p.accent_active = Color::hex(0x2563EB);
    } else {
        p.accent = Color::hex(0x2563EB);
        p.accent_hover = Color::hex(0x1D4ED8);
        p.accent_active = Color::hex(0x1E40AF);
    }
    p
}

const SIDEBAR_W: i32 = 216;

pub fn main_view(gui: &Gui) -> Element {
    let page = gui.page;
    let load_error = gui.load_error;
    let load_error_text = load_error.map(|e| format!("配置文件解析失败，修正后到「设置」页点「重新加载配置」：{e}"));

    let content = Element::col()
        .weight(1.0)
        .height_match()
        .bg_role(Role::Bg)
        .child(
            Element::label_signal(load_error_text)
                .fg_role(Role::Danger)
                .width_match()
                .padding_xy(24, 8)
                .visible_when(move || load_error.with(|e| !e.is_empty())),
        )
        .child(
            Element::stack()
                .weight(1.0)
                .width_match()
                .child(pages::overview::view(gui).visible_when(move || page.get() == 0))
                .child(pages::serial::view(gui).visible_when(move || page.get() == 1))
                .child(pages::ssh::view(gui).visible_when(move || page.get() == 2))
                .child(pages::tcp::view(gui).visible_when(move || page.get() == 3))
                .child(pages::settings::view(gui).visible_when(move || page.get() == 4)),
        );

    Element::stack()
        .fill()
        .child(
            Element::col()
                .fill()
                .child(
                    Element::row()
                        .weight(1.0)
                        .width_match()
                        .child(sidebar(gui))
                        .child(content),
                )
                .child(status_bar(gui)),
        )
        .child(gui.editors.view(gui))
}

fn sidebar(gui: &Gui) -> Element {
    let metrics = windui::theme::current().metrics.clone();
    let attention = gui.summary_attention;
    let pill_role = attention.map(|a| if *a { Role::Warning } else { Role::Success });

    let brand = Element::col()
        .spacing(6)
        .padding_xy(6, 4)
        .child(Element::label("PortTool").font_size(metrics.font_lg).font_weight(700))
        .child(
            Element::label_signal(gui.summary)
                .font_size(metrics.font_xs)
                .fg_role_signal(pill_role),
        );

    Element::col()
        .width(SIDEBAR_W)
        .height_match()
        .bg_role(Role::Surface)
        .padding(12)
        .spacing(4)
        .child(brand)
        .child(Element::label("转发服务").fg_role(Role::TextMuted).padding_xy(14, 10))
        .child(nav_item(gui, 0, "概览", None))
        .child(nav_item(gui, 1, "串口共享", Some(gui.nav_counts[0])))
        .child(nav_item(gui, 2, "SSH 转发", Some(gui.nav_counts[1])))
        .child(nav_item(gui, 3, "TCP 转发", Some(gui.nav_counts[2])))
        .child(Element::flex_spacer())
        .child(nav_item(gui, 4, "设置", None))
}

fn nav_item(gui: &Gui, index: usize, label: &str, count: Option<Signal<String>>) -> Element {
    let page = gui.page;
    let selected = move || page.get() == index;
    let text_role = page.map(move |p| if *p == index { Role::AccentText } else { Role::Text });
    let mut row = Element::row()
        .width_match()
        .cross(Align::Center)
        .padding_xy(14, 10)
        .corner(8.0)
        .clickable()
        .bg_when(move || {
            if selected() {
                windui::theme::current().palette.accent.scale_alpha(0.14)
            } else {
                Color::TRANSPARENT
            }
        })
        .on_click(move |_| page.set(index))
        .child(
            Element::label(label.to_string())
                .font_weight(if index == 4 { 400 } else { 500 })
                .fg_role_signal(text_role),
        )
        .child(Element::flex_spacer());
    if let Some(count) = count {
        row = row.child(
            Element::label_signal(count)
                .fg_role(Role::TextMuted)
                .padding_xy(8, 1)
                .corner(999.0)
                .bg_role(Role::SurfaceAlt),
        );
    }
    row
}

fn status_bar(gui: &Gui) -> Element {
    let url = gui.http_url;
    let g = gui.clone();
    let g2 = gui.clone();
    Element::row()
        .width_match()
        .height(34)
        .cross(Align::Center)
        .bg_role(Role::Surface)
        .border_role(Role::Divider, 1)
        .padding_xy(16, 0)
        .spacing(10)
        .child(Element::label("●").fg_role(Role::Success))
        .child(Element::label("HTTP 接口").fg_role(Role::TextMuted))
        .child(ui::mono(url))
        .child(Element::button("复制").small().outline().neutral().on_click(move |ctx| {
            ctx.clipboard_set(&url.get());
            ctx.toast_ok("已复制 HTTP 地址");
        }))
        .child(Element::flex_spacer())
        .child(Element::button("打开日志目录").small().outline().neutral().on_click(move |_| g.open_log_dir()))
        .child(Element::button("打开配置文件").small().outline().neutral().on_click(move |_| g2.open_config()))
}
