//! 设置页：常规（开机自启）、外观（主题）、服务（HTTP / 监听地址 / token）、文件。

use windui::prelude::*;

use super::page_body;
use crate::autostart;
use crate::gui::ui::page_header;
use crate::gui::Gui;

const FIELD_W: i32 = 300;

pub fn view(gui: &Gui) -> Element {
    let server = gui.service.config().server;
    let bind = signal(server.bind.clone());
    let http_port = signal(server.http_port.to_string());
    let token = signal(server.token.clone().unwrap_or_default());
    let buffer_lines = signal(server.buffer_lines.to_string());
    let log_dir = signal(server.log_dir.map(|p| p.display().to_string()).unwrap_or_default());
    let error = signal(String::new());

    let general = Element::card(
        "常规",
        Element::col()
            .width_match()
            .spacing(8)
            .child(Element::setting_row_desc(
                "开机自启",
                "登录 Windows 后在托盘中静默启动",
                Element::checkbox("", gui.autostart).on_toggle({
                    let autostart = gui.autostart;
                    move |ctx| {
                        let want = !autostart.get();
                        match autostart::set_enabled(want) {
                            Ok(()) => autostart.set(want),
                            Err(e) => ctx.toast_err(format!("设置开机自启失败: {e:#}")),
                        }
                    }
                }),
            ))
            .child(Element::setting_row(
                "主题",
                Element::segmented(vec!["亮色", "暗色", "跟随系统"], gui.theme_mode).on_segment_change({
                    let gui = gui.clone();
                    move |_, _| gui.apply_theme()
                }),
            )),
    );

    let save = {
        let gui = gui.clone();
        move |ctx: &mut windui::core::EventCtx| {
            let result = (|| -> Result<(), String> {
                let mut config = gui.editable_config()?;
                config.server.bind = bind.get().trim().to_string();
                config.server.http_port = http_port
                    .get()
                    .trim()
                    .parse()
                    .map_err(|_| "HTTP 端口必须是 1–65535 的整数".to_string())?;
                let t = token.get().trim().to_string();
                config.server.token = (!t.is_empty()).then_some(t);
                config.server.buffer_lines = buffer_lines
                    .get()
                    .trim()
                    .parse()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or("缓冲行数必须是正整数")?;
                let dir = log_dir.get().trim().to_string();
                config.server.log_dir = (!dir.is_empty()).then(|| dir.into());
                gui.apply_config(config).map_err(|e| format!("{e:#}"))
            })();
            match result {
                Ok(()) => {
                    error.set(String::new());
                    ctx.toast_ok("已保存并应用");
                }
                Err(e) => error.set(e),
            }
        }
    };

    let service = Element::card(
        "服务",
        Element::col()
            .width_match()
            .spacing(8)
            .child(Element::field(
                "监听地址",
                Element::combo_box(bind, vec!["0.0.0.0", "127.0.0.1"]).width(FIELD_W),
            ))
            .child(Element::label("0.0.0.0 = 局域网可访问；127.0.0.1 = 仅本机").fg_role(Role::TextMuted))
            .child(Element::field("HTTP 端口", Element::text_input(http_port, "7000").numeric().width(160)))
            .child(Element::field(
                "访问 token",
                Element::text_input(token, "留空表示不需要").password().width(FIELD_W),
            ))
            .child(Element::label("token 只保护 HTTP 接口；串口终端端口不做认证").fg_role(Role::TextMuted))
            .child(Element::field(
                "日志缓冲行数",
                Element::text_input(buffer_lines, "20000").numeric().width(160),
            ))
            .child(Element::field("日志目录", Element::text_input(log_dir, "留空使用配置文件旁的 logs/").width(FIELD_W)))
            .child(
                Element::label_signal(error)
                    .fg_role(Role::Danger)
                    .width_match()
                    .visible_when(move || error.with(|e| !e.is_empty())),
            )
            .child(
                Element::hstack()
                    .width_match()
                    .child(Element::flex_spacer())
                    .child(Element::button("保存并应用").small().on_click(save)),
            ),
    );

    let files = Element::card(
        "文件",
        Element::hstack()
            .width_match()
            .spacing(8)
            .child(Element::button("打开配置文件").small().outline().neutral().on_click({
                let gui = gui.clone();
                move |_| gui.open_config()
            }))
            .child(Element::button("打开日志目录").small().outline().neutral().on_click({
                let gui = gui.clone();
                move |_| gui.open_log_dir()
            }))
            .child(Element::button("重新加载配置").small().outline().neutral().on_click({
                let gui = gui.clone();
                move |ctx| match gui.reload_from_file() {
                    Ok(()) => ctx.toast_ok("已重新加载配置"),
                    Err(e) => ctx.toast_err(format!("{e:#}")),
                }
            })),
    );

    page_body()
        .child(page_header("设置", "应用与服务选项", vec![]))
        .child(
            Element::scroll().weight(1.0).width_match().child(
                Element::col().width_match().spacing(16).child(general).child(service).child(files),
            ),
        )
}
