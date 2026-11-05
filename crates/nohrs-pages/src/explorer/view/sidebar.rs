use nohrs_core::t;
use crate::explorer::ExplorerPane;
use crate::explorer::places::{Place, SidebarInputPurpose, THIS_PC};
use crate::explorer::tags::Tags;
use gpui::prelude::*;
use gpui::*;
use gpui_component::{Icon, IconName, Sizable as _};
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
) -> AnyElement {
    page.ensure_places(cx);
    let icon_px = ((ICON_SIZE * window.scale_factor()).round() as u32).max(16);
    let places = page.sidebar.places.clone().unwrap_or_default();
    // Docked above or below the listing the sidebar is a wide, short band:
    // a tree would show three rows of it. Each group becomes a line of
    // chips that wraps instead, its name at the start.
    if !super::panels::side(super::panels::Panel::Sidebar).is_column() {
        return render_band(page, &places, icon_px, cx).into_any_element();
    }

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
        // The network places added by hand go under Network, where a share
        // found by discovery would be.
        if place.parsing_name.eq_ignore_ascii_case(crate::explorer::network::NETWORK) {
            list = network_rows(page, list, cx);
        }
    }
    if !places.other.iter().any(|place| place.parsing_name.eq_ignore_ascii_case(crate::explorer::network::NETWORK)) {
        list = network_rows(page, list, cx);
    }
    list = tag_rows(page, list, cx);

    div()
        .id("sidebar")
        .size_full()
        .when(!page.framed.sidebar, |this| this.bg(rgb(theme::bg())))
        .py(px(8.0))
        .overflow_y_scroll()
        .child(list)
        .into_any_element()
}

// The sidebar as a band: Quick access, the drives and the other places, each
// a wrapping line of chips under its name.
fn render_band(
    page: &mut ExplorerPane,
    places: &crate::explorer::places::Places,
    icon_px: u32,
    cx: &mut Context<ExplorerPane>,
) -> impl IntoElement + use<> {
    let pc_name = places
        .this_pc
        .as_ref()
        .map(|place| place.name.clone())
        .unwrap_or_else(|| t!("Este Computador").to_string());
    let groups: [(String, Vec<Place>); 3] = [
        (t!("Acesso rápido").to_string(), places.quick_access.clone()),
        (pc_name, places.drives.clone()),
        (String::new(), places.other.clone()),
    ];
    let mut band = div().flex().flex_col().gap(px(6.0)).px(px(10.0));
    for (index, (name, members)) in groups.into_iter().enumerate() {
        if members.is_empty() {
            continue;
        }
        let chips: Vec<AnyElement> = members
            .iter()
            .map(|place| chip(page, place, index, icon_px, cx).into_any_element())
            .collect();
        band = band.child(
            div()
                .flex()
                .items_start()
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(110.0))
                        .flex_shrink_0()
                        .pt(px(6.0))
                        .text_xs()
                        .text_color(rgb(theme::muted()))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(name),
                )
                .child(div().flex_1().min_w(px(0.0)).flex().flex_wrap().gap(px(4.0)).children(chips)),
        );
    }
    let tags = Tags::all(cx);
    if !tags.is_empty() {
        let chips: Vec<AnyElement> = tags
            .into_iter()
            .map(|tag| {
                let location = crate::explorer::tags::location(tag.id);
                let active = page.cwd == location;
                let id = tag.id;
                div()
                    .id(SharedString::from(format!("tag-chip-{}", tag.id)))
                    .h(px(ROW_HEIGHT))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .rounded(px(6.0))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .cursor_pointer()
                    .when(active, |this| this.bg(rgb(theme::accent_subtle())))
                    .when(!active, |this| this.hover(|this| this.bg(rgb(theme::bg_hover()))))
                    .on_click(cx.listener(move |this, _, window, cx| this.change_dir(location.clone(), window, cx)))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.open_tag_menu(id, window, cx);
                        }),
                    )
                    .child(div().size(px(10.0)).rounded_full().bg(rgb(tag.color)))
                    .child(div().text_sm().whitespace_nowrap().text_color(rgb(theme::fg())).child(tag.name))
                    .into_any_element()
            })
            .collect();
        band = band.child(
            div()
                .flex()
                .items_start()
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(110.0))
                        .flex_shrink_0()
                        .pt(px(6.0))
                        .text_xs()
                        .text_color(rgb(theme::muted()))
                        .child(t!("Tags")),
                )
                .child(div().flex_1().min_w(px(0.0)).flex().flex_wrap().gap(px(4.0)).children(chips)),
        );
    }
    div()
        .id("sidebar")
        .size_full()
        .when(!page.framed.sidebar, |this| this.bg(rgb(theme::bg())))
        .py(px(8.0))
        .overflow_y_scroll()
        .child(band)
}

// One place as a chip of the band: its icon and name, a click opens it.
fn chip(
    page: &mut ExplorerPane,
    place: &Place,
    group: usize,
    icon_px: u32,
    cx: &mut Context<ExplorerPane>,
) -> impl IntoElement + use<> {
    let active = place
        .path
        .as_deref()
        .is_some_and(|path| crate::explorer::same_path(path, &page.cwd));
    let icon = match page.sidebar.icons.icon_in(&place.parsing_name, true, icon_px, |pane| &mut pane.sidebar.icons, cx) {
        Some(image) => img(ImageSource::Render(image)).size(px(ICON_SIZE)).flex_shrink_0().into_any_element(),
        None => Icon::new(IconName::Folder).size_4().text_color(rgb(theme::gray_500())).into_any_element(),
    };
    let opened = place.clone();
    let menu = place.clone();
    div()
        .id(SharedString::from(format!("chip-{group}-{}", place.parsing_name)))
        .h(px(ROW_HEIGHT))
        .max_w(px(220.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .border_1()
        .border_color(rgb(theme::border()))
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
        .child(icon)
        .child(
            div()
                .min_w(px(0.0))
                .text_sm()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_color(rgb(theme::fg()))
                .child(place.name.clone()),
        )
}

// The places added by hand, then the row (or field) to add another.
fn network_rows(page: &mut ExplorerPane, mut list: Div, cx: &mut Context<ExplorerPane>) -> Div {
    for address in crate::explorer::network::NetworkPlaces::all(cx) {
        let active = crate::explorer::same_path(&address, &page.cwd);
        let opened = address.clone();
        let removed = address.clone();
        list = list.child(
            simple_row(
                SharedString::from(format!("network-place-{address}")),
                Icon::new(Icon::empty()).path("icons/server.svg").size_4().text_color(rgb(theme::muted())).into_any_element(),
                crate::explorer::network::display_name(&address),
                active,
                1,
            )
            .on_click(cx.listener(move |this, _, window, cx| this.change_dir(opened.clone(), window, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_network_place_menu(removed.clone(), window, cx);
                }),
            ),
        );
    }
    let adding = page
        .sidebar
        .input
        .as_ref()
        .filter(|field| field.purpose == SidebarInputPurpose::NetworkPlace)
        .map(|field| field.input.clone());
    match adding {
        Some(input) => list.child(field_row(input, 1)),
        None => list.child(
            simple_row(
                "network-add",
                Icon::new(IconName::Plus).size_4().text_color(rgb(theme::muted())).into_any_element(),
                t!("Adicionar local de rede").to_string(),
                false,
                1,
            )
            .text_color(rgb(theme::muted()))
            .on_click(cx.listener(|this, _, window, cx| {
                this.begin_sidebar_input(SidebarInputPurpose::NetworkPlace, String::new(), window, cx)
            })),
        ),
    }
}

// The tags, each with its colour and how many items carry it; a click lists
// them. A tag being renamed is a field in its place.
fn tag_rows(page: &mut ExplorerPane, mut list: Div, cx: &mut Context<ExplorerPane>) -> Div {
    let tags = Tags::all(cx);
    let counts = Tags::counts(cx);
    list = list.child(div().my(px(8.0)).mx(px(6.0)).h(px(1.0)).bg(rgb(theme::border()))).child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(24.0))
            .pl(px(8.0))
            .pr(px(4.0))
            .text_xs()
            .text_color(rgb(theme::muted()))
            .child(t!("Tags"))
            .child(
                div()
                    .id("tag-new")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(20.0))
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(theme::bg_hover())))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let name = t!("Nova tag").to_string();
                        let id = Tags::create(name.clone(), &[], cx);
                        this.begin_sidebar_input(SidebarInputPurpose::RenameTag(id), name, window, cx);
                    }))
                    .child(Icon::new(IconName::Plus).size_3()),
            ),
    );
    for tag in tags {
        let renaming = page
            .sidebar
            .input
            .as_ref()
            .filter(|field| field.purpose == SidebarInputPurpose::RenameTag(tag.id))
            .map(|field| field.input.clone());
        if let Some(input) = renaming {
            list = list.child(field_row(input, 0));
            continue;
        }
        let location = crate::explorer::tags::location(tag.id);
        let active = page.cwd == location;
        let count = counts.get(&tag.id).copied().unwrap_or(0);
        let id = tag.id;
        list = list.child(
            simple_row(
                SharedString::from(format!("tag-{}", tag.id)),
                div().size(px(10.0)).mx(px(3.0)).rounded_full().bg(rgb(tag.color)).into_any_element(),
                tag.name.clone(),
                active,
                0,
            )
            .when(count > 0, |this| {
                this.child(div().flex_shrink_0().text_xs().text_color(rgb(theme::muted())).child(count.to_string()))
            })
            .on_click(cx.listener(move |this, _, window, cx| this.change_dir(location.clone(), window, cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_tag_menu(id, window, cx);
                }),
            ),
        );
    }
    list
}

// A sidebar row with no tree around it: an icon and a name.
fn simple_row(
    id: impl Into<ElementId>,
    icon: AnyElement,
    name: String,
    active: bool,
    depth: usize,
) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(6.0))
        .pl(px(2.0 + CHEVRON_WIDTH + depth as f32 * CHEVRON_WIDTH))
        .pr(px(8.0))
        .rounded(px(4.0))
        .cursor_pointer()
        .when(active, |this| this.bg(rgb(theme::accent_subtle())))
        .when(!active, |this| this.hover(|this| this.bg(rgb(theme::bg_hover()))))
        .child(div().flex_shrink_0().child(icon))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_sm()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .text_color(rgb(theme::fg()))
                .child(name),
        )
}

// A field in the sidebar, as tall as a row.
fn field_row(input: Entity<gpui_component::input::InputState>, depth: usize) -> impl IntoElement {
    div()
        .w_full()
        .pl(px(2.0 + depth as f32 * CHEVRON_WIDTH))
        .pr(px(4.0))
        .py(px(2.0))
        .child(gpui_component::input::Input::new(&input).small())
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
