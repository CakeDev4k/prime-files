use nohrs_core::t;
use crate::explorer::ExplorerPane;
use crate::explorer::places::{Place, THIS_PC};
use gpui::prelude::*;
use gpui::*;
use gpui_component::{Icon, IconName};
use nohrs_ui::theme::theme;

const ROW_HEIGHT: f32 = 28.0;
const ICON_SIZE: f32 = 16.0;
/// Width of the expand chevron's column; each tree level indents by it.
const CHEVRON_WIDTH: f32 = 16.0;

/// Renders the sidebar the way Explorer's navigation pane is laid out: Quick
/// access, then This PC as a tree of drives and folders, then Network and
/// Linux. Names and icons come from the Shell, so they match Explorer's.
pub fn render(
    page: &mut ExplorerPane,
    window: &mut Window,
    cx: &mut Context<ExplorerPane>,
) -> impl IntoElement + use<> {
    page.ensure_places(cx);
    let icon_px = ((ICON_SIZE * window.scale_factor()).round() as u32).max(16);
    let places = page.sidebar.places.clone().unwrap_or_default();

    let mut list = div().flex().flex_col().px(px(6.0)).gap(px(1.0));
    for place in &places.quick_access {
        list = list.child(row(page, place, 0, None, icon_px, cx));
    }
    list = list.child(
        div()
            .my(px(8.0))
            .mx(px(6.0))
            .h(px(1.0))
            .bg(rgb(theme::border())),
    );

    let this_pc = places.this_pc.clone().unwrap_or_else(|| Place {
        name: t!("Este Computador").to_string(),
        path: None,
        parsing_name: THIS_PC.to_string(),
        pinned: false,
    });
    let pc_open = page.sidebar.expanded.contains(THIS_PC);
    list = list.child(row(page, &this_pc, 0, Some(pc_open), icon_px, cx));
    if pc_open {
        for drive in &places.drives {
            list = tree(page, list, drive, 1, icon_px, cx);
        }
    }
    for place in &places.other {
        list = list.child(row(page, place, 0, None, icon_px, cx));
    }

    div()
        .id("sidebar")
        .size_full()
        .when(!page.framed.sidebar, |this| this.bg(rgb(theme::bg())))
        .py(px(8.0))
        .overflow_y_scroll()
        .child(list)
}

// A node and, when open, everything under it.
fn tree(
    page: &mut ExplorerPane,
    mut list: Div,
    place: &Place,
    depth: usize,
    icon_px: u32,
    cx: &mut Context<ExplorerPane>,
) -> Div {
    let open = page.sidebar.expanded.contains(&place.parsing_name);
    list = list.child(row(page, place, depth, Some(open), icon_px, cx));
    if open
        && let Some(path) = &place.path
        && let Some(children) = page.sidebar.children.get(path).cloned()
    {
        for child in &children {
            list = tree(page, list, child, depth + 1, icon_px, cx);
        }
    }
    list
}

// One row. `expandable` is `Some(open)` for a tree node, `None` for a leaf
// (Quick access entries are links, as in Explorer).
fn row(
    page: &mut ExplorerPane,
    place: &Place,
    depth: usize,
    expandable: Option<bool>,
    icon_px: u32,
    cx: &mut Context<ExplorerPane>,
) -> impl IntoElement + use<> {
    let active = place
        .path
        .as_deref()
        .is_some_and(|path| crate::explorer::same_path(path, &page.cwd));
    let icon = page.sidebar.icons.icon_in(
        &place.parsing_name,
        // Places are folders, drives and shell locations, each with its own.
        true,
        icon_px,
        |pane| &mut pane.sidebar.icons,
        cx,
    );
    let icon = match icon {
        Some(image) => img(ImageSource::Render(image))
            .size(px(ICON_SIZE))
            .flex_shrink_0()
            .into_any_element(),
        None => Icon::new(IconName::Folder)
            .size_4()
            .text_color(rgb(theme::gray_500()))
            .into_any_element(),
    };
    let chevron = match expandable {
        Some(open) => {
            let toggled = place.clone();
            div()
                .id(SharedString::from(format!("toggle-{}", place.parsing_name)))
                .w(px(CHEVRON_WIDTH))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.toggle_place(&toggled, cx);
                    }),
                )
                .child(
                    Icon::new(if open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size_3()
                    .text_color(rgb(theme::gray_500())),
                )
                .into_any_element()
        }
        None => div().w(px(CHEVRON_WIDTH)).flex_shrink_0().into_any_element(),
    };
    let opened = place.clone();
    let menu = place.clone();
    let drive_space = place.path.as_deref().and_then(|path| crate::explorer::drives::space(path, cx));
    div()
        .id(SharedString::from(format!("place-{depth}-{}", place.parsing_name)))
        .w_full()
        .h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(6.0))
        .pl(px(2.0 + depth as f32 * CHEVRON_WIDTH))
        .pr(px(8.0))
        .rounded(px(4.0))
        .cursor_pointer()
        .when(active, |this| this.bg(rgb(theme::accent_subtle())))
        .when(!active, |this| this.hover(|this| this.bg(rgb(theme::bg_hover()))))
        .on_click(cx.listener(move |this, _, window, cx| this.open_place(&opened, window, cx)))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_place_menu(menu.clone(), window, cx);
            }),
        )
        .child(chevron)
        .child(icon)
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_sm()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_color(rgb(theme::fg()))
                .child(place.name.clone()),
        )
        .when_some(drive_space, |this, space| {
            // How full the drive is, beside its name: a glance at C: and D:
            // without opening This PC.
            this.child(
                div()
                    .id(SharedString::from(format!("drive-space-{}", place.parsing_name)))
                    .w(px(34.0))
                    .flex_shrink_0()
                    .child(crate::explorer::view::listing::usage_bar(Some(space), 4.0))
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(space.describe()).build(window, cx)
                    }),
            )
        })
        .when(place.pinned, |this| {
            this.child(
                Icon::new(Icon::empty())
                    .path("icons/pin.svg")
                    .size_3()
                    .flex_shrink_0()
                    .text_color(rgb(theme::gray_500())),
            )
        })
}
