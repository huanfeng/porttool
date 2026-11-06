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
const W_ACTIVITY: f32 = 1.2;
const W_STATUS: i32 = 128;
const W_OPS: i32 = 104;
const ROW_H: i32 = 44;

/// 路径区里一格显示 `RuleRow` 的什么：整串路径，或路径的第几段。
#[derive(Clone, Copy)]
enum Field {
    Path,
    Part(usize),
}

impl Field {
    fn of(self, r: &RuleRow) -> &str {
        match self {
            Field::Path => &r.path,
            Field::Part(i) => r.parts.get(i).map(String::as_str).unwrap_or(""),
        }
    }
}

/// 列表的列：`kind` 为 `None` 是概览（混合类型，一整列路径）；类型页按语义分列，每格各自截断，
/// 列标题就是说明，不再需要箭头。返回（路径区的列：标题、字段、宽度权重；是否有「连接 / 流量」列）。
/// SSH 没有连接数和流量数据，那一列不要，把宽度让给路径。
fn columns(kind: Option<Kind>) -> (&'static [(&'static str, Field, f32)], bool) {
    match kind {
        None => (&[("路径", Field::Path, 5.0)], true),
        Some(Kind::Serial) => (&[("本机串口", Field::Part(0), 2.5), ("终端连接", Field::Part(1), 2.5)], true),
        Some(Kind::Ssh) => (
            &[("本机监听", Field::Part(0), 2.2), ("SSH 服务器", Field::Part(1), 2.5), ("目标", Field::Part(2), 2.6)],
            false,
        ),
        Some(Kind::Tcp) => (&[("本机监听", Field::Part(0), 2.5), ("目标", Field::Part(1), 2.5)], true),
    }
}

/// 名称列的宽度权重：概览页的名称前面还有类型徽章，要宽一些。表头与数据行必须一致。
fn name_weight(kind: Option<Kind>) -> f32 {
    match kind {
        None => 2.3,
        // SSH 的名称前面还有 -L / -R 标签，而且没有「连接 / 流量」列，宽度够
        Some(Kind::Ssh) => 2.0,
        Some(_) => 1.5,
    }
}

/// 列表顶部的列标题（只出现一次）。
pub fn header(kind: Option<Kind>) -> Element {
    let cell = |text: &str| Element::label(text.to_string()).fg_role(Role::TextMuted);
    let (cols, activity) = columns(kind);
    let mut row = Element::row()
        .width_match()
        .bg_role(Role::SurfaceAlt)
        .padding_xy(16, 8)
        .spacing(12)
        .child(cell("名称").weight(name_weight(kind)));
    for (title, _, weight) in cols {
        row = row.child(cell(title).weight(*weight));
    }
    row = row.child(cell("状态").width(W_STATUS));
    if activity {
        row = row.child(cell("连接 / 流量").weight(W_ACTIVITY));
    }
    row.child(cell("操作").width(W_OPS).text_align(Align::End))
}

/// 一条规则。`kind` 为 `None`（概览页）时名称前面带类型徽章，用来区分类型。
pub fn rule_row(gui: &Gui, r: RuleRow, kind: Option<Kind>) -> Element {
    let metrics = gui.metric_signals(&r.name);
    let (cols, activity) = columns(kind);

    let mut name = Element::hstack().weight(name_weight(kind)).cross(Align::Center).spacing(8);
    if kind.is_none() {
        name = name.child(kind_badge(r.kind));
    }
    // SSH 的方向标签放在名称前面，和名称挨着；名称标签占徽章之外的剩余宽度，
    // 放不下时省略，不盖住旁边的路径列
    if !r.tag.is_empty() {
        name = name.child(Element::badge_intent(r.tag.clone(), Intent::Success).tooltip(r.sub.clone()));
    }
    name = name.child(
        Element::label(r.name.clone())
            .font_weight(600)
            .max_lines(1)
            .truncate(Truncate::End)
            .weight(1.0),
    );

    let mut main = Element::row()
        .width_match()
        .height(ROW_H)
        .cross(Align::Center)
        .padding_xy(16, 0)
        .spacing(12)
        .child(name);
    for (_, field, weight) in cols {
        let text = field.of(&r).to_string();
        main = main.child(
            mono(text.clone())
                .max_lines(1)
                // 超长时保留开头和结尾（地址的开头、端口），省略中间；完整内容在 tooltip 里
                .truncate(Truncate::Middle)
                .tooltip(text)
                .weight(*weight),
        );
    }
    main = main.child(
        Element::hstack()
            .width(W_STATUS)
            .child(status_pill(&r.status, r.tone).tooltip(r.detail.clone())),
    );
    if activity {
        main = main.child(
            Element::label(metrics.activity)
                .fg_role(Role::TextMuted)
                .max_lines(1)
                .truncate(Truncate::End)
                .weight(W_ACTIVITY),
        );
    }
    let main = main.child(
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
pub fn rule_list(gui: &Gui, rows: Signal<Vec<RuleRow>>, kind: Option<Kind>) -> Element {
    let g = gui.clone();
    Element::list_signal(rows, |r: &RuleRow| r.name.clone(), move |r: RuleRow| rule_row(&g, r, kind))
        .weight(1.0)
}

/// 列表顶部的工具栏：搜索、（概览页）类型筛选、状态筛选、「选项」菜单（异常优先、批量启停），
/// 类型页右侧是统计文字（概览页上方的统计卡片已经有了）。
/// `view` 是这个列表在 `Gui::views` 里的下标；每个页面的输入互相独立。
pub fn toolbar(gui: &Gui, view: usize) -> Element {
    let state = gui.views[view];
    // 控件放在流式容器里，窗口窄时折行，不会被挤出去
    let mut controls = Element::flow()
        .spacing(12)
        .run_spacing(8)
        .weight(1.0)
        .child(Element::text_input(state.query, "搜索名称、端口、状态").leading_icon('\u{1F50D}').width(180));
    if view == 0 {
        controls = controls.child(Element::segmented(vec!["全部", "串口", "SSH", "TCP"], state.kind));
    }
    controls = controls
        .child(Element::segmented(vec!["全部", "异常", "已停用"], state.status))
        .child(options_menu(gui, view));
    let mut bar = Element::row()
        .width_match()
        .cross(Align::Center)
        .padding(12)
        .spacing(12)
        .child(controls);
    if view != 0 {
        bar = bar.child(Element::label_signal(state.summary).fg_role(Role::TextMuted));
    }
    bar
}

/// 「选项 ▾」：异常优先排序开关，以及全部启用 / 全部停用（一次写配置、一次重载）。
fn options_menu(gui: &Gui, view: usize) -> Element {
    let state = gui.views[view];
    // 批量启停的目标在点击时才取：概览页是此刻筛选出来的规则
    let batch = |label: &'static str, enabled: bool| {
        let gui = gui.clone();
        CheckMenuItem::action(label, move |ctx| {
            let names = gui.batch_targets(view);
            match gui.set_enabled_many(&names, enabled) {
                Ok(0) => ctx.toast("没有需要改动的规则"),
                Ok(changed) => ctx.toast_ok(format!("已{} {changed} 条规则", if enabled { "启用" } else { "停用" })),
                Err(e) => ctx.toast_err(e),
            }
        })
    };
    Element::check_menu(
        "选项",
        vec![
            CheckMenuItem::check("异常优先", state.problems_first),
            CheckMenuItem::separator(),
            batch("全部启用", true),
            batch("全部停用", false),
        ],
    )
    .width(110)
}

/// 列表为空时的提示：一条规则都没有时给出添加入口，有规则但被筛选为空时说明没有匹配。
pub fn empty_state(gui: &Gui, view: usize, hint: &str, add: Element) -> Element {
    let state = gui.views[view];
    let (total, shown) = (state.total, state.shown);
    Element::col()
        .width_match()
        .child(
            Element::row()
                .width_match()
                .cross(Align::Center)
                .padding(24)
                .spacing(12)
                .child(Element::label(hint.to_string()).fg_role(Role::TextMuted))
                .child(add)
                .visible_when(move || total.get() == 0),
        )
        .child(
            Element::label("没有匹配的规则")
                .fg_role(Role::TextMuted)
                .width_match()
                .padding(24)
                .visible_when(move || total.get() > 0 && shown.with(|s| s.is_empty())),
        )
}

/// 串口 / SSH / TCP 页：页头 + 一张带工具栏的规则表。
pub fn type_page(gui: &Gui, kind: Kind) -> Element {
    let (view, title, subtitle, add_label, empty) = match kind {
        Kind::Serial => (
            1,
            "串口共享",
            "把本机串口通过 TCP 共享给局域网内的终端",
            "＋ 添加串口",
            "还没有共享的串口。",
        ),
        Kind::Ssh => (
            2,
            "SSH 转发",
            "通过 SSH 服务器建立本地或远程端口转发（调用系统 ssh）",
            "＋ 添加转发",
            "还没有 SSH 转发（系统需要已安装 OpenSSH 客户端）。",
        ),
        Kind::Tcp => (
            3,
            "TCP 转发",
            "把本机端口的流量转发到另一个地址",
            "＋ 添加转发",
            "还没有 TCP 转发。",
        ),
    };
    let open_add = {
        let gui = gui.clone();
        move |_: &mut windui::core::EventCtx| gui.editors.open(&gui, kind, None)
    };
    let add = Element::button(add_label).on_click(open_add.clone());
    let add_inline = Element::button(add_label).small().outline().on_click(open_add);
    page_body().child(page_header(title, subtitle, vec![add])).child(
        panel(
            Element::col()
                .width_match()
                .height_match()
                .child(toolbar(gui, view))
                .child(header(Some(kind)))
                .child(empty_state(gui, view, empty, add_inline))
                .child(rule_list(gui, gui.views[view].shown, Some(kind))),
        )
        .weight(1.0),
    )
}
