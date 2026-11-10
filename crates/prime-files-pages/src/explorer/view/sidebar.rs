use prime_files_core::config::{DriveLabel, DriveUsage, SidebarDensity, SidebarLook, SidebarSection};
use prime_files_core::t;
use crate::explorer::ExplorerPane;
use crate::explorer::bookmarks::{BookmarkGroup, Bookmarks};
use crate::explorer::places::{Place, Places, SidebarInputPurpose, THIS_PC};
use crate::explorer::tags::Tags;
use gpui::prelude::*;
use gpui::*;
use gpui_component::{Icon, IconName, Sizable as _};
use prime_files_ui::theme::theme;

const ICON_SIZE: f32 = 16.0;
/// Width of the expand chevron's column; each tree level indents by it.
const CHEVRON_WIDTH: f32 = 16.0;
/// How much taller a drive's row is with its usage bar under the name.
const BAR_BELOW: f32 = 6.0;

/// The parts of `[sidebar]` the rows are drawn by.
#[derive(Clone, Copy)]
struct Draw {
    row_height: f32,
    icons_only: bool,
    tree: bool,
    tree_lines: bool,
    headers: bool,
    drive_usage: DriveUsage,
    drive_label: DriveLabel,
    cards: bool,
}

impl Draw {
    fn of(look: &SidebarLook) -> Self {
        Self {
            row_height: match look.density {
                SidebarDensity::Compact => 22.0,
                SidebarDensity::Normal => 28.0,
                SidebarDensity::Comfortable => 34.0,
            },
            icons_only: look.icons_only,
            // A strip of icons has no room for levels.
            tree: look.tree && !look.icons_only,
            tree_lines: look.tree_lines,
            headers: look.headers,
            drive_usage: if look.icons_only { DriveUsage::None } else { look.drive_usage },
            drive_label: look.drive_label,
            cards: look.cards && !look.icons_only,
        }
    }

    // The room a leaf leaves before its icon: the chevron's column in a
    // tree, so the names line up with the nodes', and nothing in a list.
    fn leaf_indent(self) -> f32 {
        if self.tree { CHEVRON_WIDTH } else { 0.0 }
    }
}

/// Whether `place` is a drive's root (`C:\`).
fn is_drive(place: &Place) -> bool {
    place.path.as_deref().is_some_and(|path| path.len() == 3 && path.ends_with(":\\"))
}

/// A drive's name as `drive_label` asks: Explorer's "Disco Local (C:)", or
/// "C: Disco Local", or "C:". A name without the letter in brackets (the
/// Shell's, for some drives) is left alone but for the letter alone.
fn drive_name(place: &Place, label: DriveLabel) -> String {
    let Some(letter) = place.path.as_deref().and_then(|path| path.get(..2)) else {
        return place.name.clone();
    };
    let bare = place
        .name
        .strip_suffix(&format!("({letter})"))
        .map(str::trim_end)
        .filter(|bare| !bare.is_empty());
    match label {
        DriveLabel::Name => place.name.clone(),
        DriveLabel::Letter => letter.to_string(),
        DriveLabel::LetterFirst => match bare {
            Some(bare) => format!("{letter} {bare}"),
            None => place.name.clone(),
        },
    }
}

/// Renders the sidebar by `[sidebar]`: the groups its style lists, in its
/// order, each as Explorer, File Pilot, One Commander or Finder draws it.
/// Names and icons come from the Shell, so they match Explorer's.
pub fn render(
    page: &mut ExplorerPane,
    window: &mut Window,
    cx: &mut Context<ExplorerPane>,
) -> AnyElement {
    page.ensure_places(cx);
    let look = crate::explorer::places::look();
    let draw = Draw::of(&look);
    let icon_px = ((ICON_SIZE * window.scale_factor()).round() as u32).max(16);
    let places = page.sidebar.places.clone().unwrap_or_default();
    // Docked above or below the listing the sidebar is a wide, short band:
    // a tree would show three rows of it. Each group becomes a line of
    // chips that wraps instead, its name at the start.
    if !super::panels::side(super::panels::Panel::Sidebar).is_column() {
        return render_band(page, &places, &look, icon_px, cx).into_any_element();
    }
    if draw.tree
        && look.follow_folder
        && look.sections.iter().any(|section| matches!(section, SidebarSection::ThisPc | SidebarSection::Drives))
    {
        page.follow_current_folder(cx);
    }

    let mut list = div()
        .flex()
        .flex_col()
        .px(px(if draw.icons_only { 4.0 } else { 6.0 }))
        .gap(px(if draw.cards { 6.0 } else { 1.0 }));
    let mut first = true;
    let body = || div().flex().flex_col().gap(px(1.0));
    for section in &look.sections {
        if section_is_empty(*section, &places, &look, cx) {
            continue;
        }
        let mut bodies = Vec::new();
        if *section == SidebarSection::Bookmarks {
            for group in Bookmarks::groups(cx) {
                bodies.push(bookmark_group(page, body(), &group, draw, &mut first, icon_px, cx));
            }
        } else {
            bodies.push(render_section(page, body(), *section, &places, &look, draw, &mut first, icon_px, cx));
        }
        for group in bodies {
            list = list.child(if draw.cards { card(group) } else { group });
        }
        // One Commander's button, right under the groups. At the end of the
        // whole bar (under Drives) the group it made, and its name field,
        // came up far above it, often out of sight: the click looked like it
        // did nothing (the owner, 09/11/2026). Here the field opens just
        // over the button that was clicked.
        if *section == SidebarSection::Bookmarks && !draw.icons_only {
            list = list.child(new_group_button(draw, cx));
        }
    }

    // A bar to grab, shown while the pointer is over the sidebar: it only
    // scrolled by the wheel, and Paullo's wheel skips (07/11/2026). Drawn
    // over the rows' right edge, only while the pointer is there.
    div()
        .relative()
        .size_full()
        .when(!page.framed.sidebar, |this| this.bg(rgb(theme::bg())))
        .child(
            div()
                .id("sidebar")
                .size_full()
                .py(px(8.0))
                .overflow_y_scroll()
                .track_scroll(&page.sidebar_scroll)
                .child(list),
        )
        .child(
            gpui_component::scroll::Scrollbar::vertical(&page.sidebar_scroll)
                .id("sidebar-scrollbar")
                .scrollbar_show(gpui_component::scroll::ScrollbarShow::Hover),
        )
        .into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn render_section(
    page: &mut ExplorerPane,
    mut list: Div,
    section: SidebarSection,
    places: &Places,
    look: &SidebarLook,
    draw: Draw,
    first: &mut bool,
    icon_px: u32,
    cx: &mut Context<ExplorerPane>,
) -> Div {
    // A flat list of places under a name.
    let links = |page: &mut ExplorerPane,
                     mut list: Div,
                     key: &'static str,
                     title: &'static str,
                     labeled: bool,
                     members: &[Place],
                     first: &mut bool,
                     cx: &mut Context<ExplorerPane>| {
        if members.is_empty() {
            return list;
        }
        let (started, heading, open) =
            begin_group(list, key, title.to_string(), labeled, Some(section), draw, first, cx);
        list = started.children(heading);
        if open {
            for place in members {
                let spec = RowSpec { prefix: key, depth: 0, expandable: None, menu: RowMenu::Place };
                list = list.child(row(page, place, spec, draw, icon_px, cx));
            }
        }
        list
    };
    match section {
        SidebarSection::QuickAccess => {
            links(page, list, "quick-access", t!("Acesso rápido"), false, &places.quick_access, first, cx)
        }
        // Favorite files under a heading of their own, apart from the
        // folders (issue #24).
        SidebarSection::Favorites => links(page, list, "favorites", t!("Favoritos"), true, &places.favorites, first, cx),
        SidebarSection::UserFolders => {
            links(page, list, "user-folders", t!("Pastas pessoais"), true, &places.user_folders, first, cx)
        }
        SidebarSection::Recent => {
            let recent: Vec<Place> = Bookmarks::recent(cx)
                .into_iter()
                .take(look.recent_count as usize)
                .map(|path| folder_place(&path, None))
                .collect();
            if recent.is_empty() {
                return list;
            }
            let (started, heading, open) =
                begin_group(list, "recent", t!("Recentes").to_string(), true, Some(section), draw, first, cx);
            list = started.children(heading);
            if open {
                for place in &recent {
                    let menu = RowMenu::Recent(place.parsing_name.clone());
                    let spec = RowSpec { prefix: "recent", depth: 0, expandable: None, menu };
                    list = list.child(row(page, place, spec, draw, icon_px, cx));
                }
            }
            list
        }
        // Drawn by `render`, a body (or card) per group.
        SidebarSection::Bookmarks => list,
        SidebarSection::Drives => {
            if places.drives.is_empty() {
                return list;
            }
            let (started, heading, open) =
                begin_group(list, "drives", t!("Discos").to_string(), true, Some(section), draw, first, cx);
            list = started.children(heading);
            if open {
                for drive in &places.drives {
                    list = tree(page, list, drive, "drives", 0, draw, icon_px, cx);
                }
            }
            list
        }
        SidebarSection::ThisPc => {
            // This PC is its own heading: the node folds its drives.
            let (started, _, _) =
                begin_group(list, "this-pc", String::new(), false, None, Draw { headers: false, ..draw }, first, cx);
            list = started;
            let this_pc = places.this_pc.clone().unwrap_or_else(|| Place {
                name: t!("Este Computador").to_string(),
                path: None,
                parsing_name: THIS_PC.to_string(),
                pinned: false,
            });
            let pc_open = draw.icons_only || page.sidebar.expanded.contains(THIS_PC);
            let expandable = (!draw.icons_only).then_some(pc_open);
            let spec = RowSpec { prefix: "this-pc", depth: 0, expandable, menu: RowMenu::Place };
            list = list.child(row(page, &this_pc, spec, Draw { tree: true, ..draw }, icon_px, cx));
            if pc_open {
                for drive in &places.drives {
                    list = tree(page, list, drive, "this-pc", 1, Draw { tree: true, ..draw }, icon_px, cx);
                }
            }
            list
        }
        SidebarSection::Network => {
            let has_network = places
                .other
                .iter()
                .any(|place| place.parsing_name.eq_ignore_ascii_case(crate::explorer::network::NETWORK));
            let (started, heading, open) =
                begin_group(list, "network", t!("Locais").to_string(), false, Some(section), draw, first, cx);
            list = started.children(heading);
            if !open {
                return list;
            }
            for place in &places.other {
                let spec = RowSpec { prefix: "network", depth: 0, expandable: None, menu: RowMenu::Place };
                list = list.child(row(page, place, spec, draw, icon_px, cx));
                // The network places added by hand go under Network, where a
                // share found by discovery would be.
                if place.parsing_name.eq_ignore_ascii_case(crate::explorer::network::NETWORK) {
                    list = network_rows(page, list, draw, cx);
                }
            }
            if !has_network {
                list = network_rows(page, list, draw, cx);
            }
            list
        }
        SidebarSection::Tags => {
            if crate::explorer::tags::in_sidebar() {
                list = tag_rows(page, list, draw, first, cx);
            }
            list
        }
    }
}

// Whether `section` has nothing to show: it is left out, so no empty title
// or card stands for it.
fn section_is_empty(section: SidebarSection, places: &Places, look: &SidebarLook, cx: &App) -> bool {
    match section {
        SidebarSection::QuickAccess => places.quick_access.is_empty(),
        SidebarSection::Favorites => places.favorites.is_empty(),
        SidebarSection::UserFolders => places.user_folders.is_empty(),
        SidebarSection::Drives => places.drives.is_empty(),
        SidebarSection::Recent => look.recent_count == 0 || Bookmarks::recent(cx).is_empty(),
        SidebarSection::Tags => !crate::explorer::tags::in_sidebar(),
        SidebarSection::Bookmarks | SidebarSection::ThisPc | SidebarSection::Network => false,
    }
}

// A group in a card of its own, as One Commander's.
fn card(body: Div) -> Div {
    body.p(px(4.0))
        .rounded(px(8.0))
        .bg(rgb(theme::bg_secondary()))
        .border_1()
        .border_color(rgb(theme::border()))
}

// "New group", under the groups: makes one and opens its name field.
fn new_group_button(draw: Draw, cx: &mut Context<ExplorerPane>) -> Stateful<Div> {
    div()
        .id("bookmark-group-new")
        .mt(px(if draw.cards { 0.0 } else { 8.0 }))
        .h(px(32.0))
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .overflow_hidden()
        .whitespace_nowrap()
        .rounded(px(8.0))
        .cursor_pointer()
        .text_sm()
        .text_color(rgb(theme::fg_secondary()))
        .when(draw.cards, |this| this.bg(rgb(theme::bg_secondary())).border_1().border_color(rgb(theme::border())))
        .hover(|style| style.bg(rgb(theme::bg_hover())).text_color(rgb(theme::fg())))
        .on_click(cx.listener(|this, _, window, cx| this.new_bookmark_group(None, window, cx)))
        .child(Icon::new(IconName::Plus).size_3())
        .child(t!("Novo grupo"))
}

// The "⋯" a heading carries: the group's menu, as One Commander's.
fn dots_button(
    id: impl Into<ElementId>,
    cx: &mut Context<ExplorerPane>,
    pressed: impl Fn(&mut ExplorerPane, &mut Window, &mut Context<ExplorerPane>) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(20.0))
        .rounded(px(4.0))
        .cursor_pointer()
        .text_color(rgb(theme::muted()))
        .hover(|style| style.bg(rgb(theme::bg_hover())).text_color(rgb(theme::fg())))
        .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new(t!("Mais opções")).build(window, cx))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                pressed(this, window, cx);
            }),
        )
        .child(Icon::new(Icon::empty()).path("icons/ellipsis.svg").size_3())
}

/// A place of the sidebar being dragged: to a group, or, from a group, to
/// another place in it or in another group.
#[derive(Clone)]
pub(crate) struct SidebarDrag {
    place: Place,
    // The group and row it was dragged out of, when it is a group's.
    from: Option<(u32, usize)>,
}

/// A group of bookmarks being dragged by its name, to another group's name.
#[derive(Clone)]
pub(crate) struct GroupDrag {
    id: u32,
    name: String,
}

// What follows the pointer while a place or a group is dragged.
struct DragChip {
    name: String,
}

impl Render for DragChip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(10.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .bg(rgb(theme::bg_secondary()))
            .border_1()
            .border_color(rgb(theme::accent()))
            .text_sm()
            .text_color(rgb(theme::fg()))
            .child(self.name.clone())
    }
}

/// A folder of the sidebar's own lists (recent, bookmarks) as a place.
fn folder_place(path: &str, name: Option<String>) -> Place {
    Place {
        name: name.unwrap_or_else(|| crate::explorer::bookmarks::folder_name(path)),
        path: Some(path.to_string()),
        parsing_name: path.to_string(),
        pinned: false,
    }
}

// Starts a group. With `headers`, its name, which a click folds (the fold
// is kept across runs); without, a line from the group above and, for the
// groups that are lists of the person's own (`labeled`), their name small
// above them, as Explorer's Favorites. A strip of icons gets only the line.
// Returns the list, the heading for the caller to add its buttons to, and
// whether the group's rows show.
//
// A built-in `section`'s heading carries the "⋯" (and the right click) that
// moves or hides it; a group of bookmarks adds its own.
#[allow(clippy::too_many_arguments)]
fn begin_group(
    mut list: Div,
    key: &str,
    title: String,
    labeled: bool,
    section: Option<SidebarSection>,
    draw: Draw,
    first: &mut bool,
    cx: &mut Context<ExplorerPane>,
) -> (Div, Option<Stateful<Div>>, bool) {
    // In cards, the cards keep the groups apart: no line, no margin.
    let was_first = std::mem::replace(first, false) || draw.cards;
    let line = || div().my(px(6.0)).mx(px(6.0)).h(px(1.0)).flex_shrink_0().bg(rgb(theme::border()));
    if draw.icons_only {
        if !was_first {
            list = list.child(line());
        }
        return (list, None, true);
    }
    if draw.headers {
        let folded = Bookmarks::is_folded(key, cx);
        let toggled = key.to_string();
        let heading = div()
            .id(SharedString::from(format!("heading-{key}")))
            .h(px(24.0))
            .when(!was_first, |this| this.mt(px(8.0)))
            .pl(px(4.0))
            .pr(px(4.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .rounded(px(4.0))
            .cursor_pointer()
            .text_xs()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(theme::muted()))
            .hover(|style| style.bg(rgb(theme::bg_hover())))
            .on_click(cx.listener(move |_, _, _, cx| Bookmarks::toggle_folded(&toggled, cx)))
            .child(
                Icon::new(if folded { IconName::ChevronRight } else { IconName::ChevronDown })
                    .size_3()
                    .text_color(rgb(theme::gray_500())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(title),
            )
            .when_some(section, |this, section| {
                this.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_section_menu(section, window, cx);
                    }),
                )
                .child(dots_button(
                    SharedString::from(format!("section-menu-{}", section.as_str())),
                    cx,
                    move |this, window, cx| this.open_section_menu(section, window, cx),
                ))
            });
        return (list, Some(heading), !folded);
    }
    if !labeled {
        if !was_first {
            list = list.child(line());
        }
        return (list, None, true);
    }
    let heading = div()
        .id(SharedString::from(format!("heading-{key}")))
        .h(px(24.0))
        .when(!was_first, |this| this.mt(px(8.0)))
        .pl(px(8.0))
        .pr(px(4.0))
        .flex()
        .items_center()
        .rounded(px(4.0))
        .text_xs()
        .text_color(rgb(theme::muted()))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(title),
        )
        .when_some(section, |this, section| {
            this.on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_section_menu(section, window, cx);
                }),
            )
        });
    (list, Some(heading), true)
}

// The small "+" a heading carries. Pressed on the button's own mouse down,
// so the heading under it does not fold.
fn plus_button(
    id: impl Into<ElementId>,
    tip: &'static str,
    cx: &mut Context<ExplorerPane>,
    pressed: impl Fn(&mut ExplorerPane, &mut Window, &mut Context<ExplorerPane>) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(20.0))
        .rounded(px(4.0))
        .cursor_pointer()
        .text_color(rgb(theme::muted()))
        .hover(|style| style.bg(rgb(theme::bg_hover())).text_color(rgb(theme::fg())))
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip).build(window, cx))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                pressed(this, window, cx);
            }),
        )
        .child(Icon::new(IconName::Plus).size_3())
}

// A group of bookmarks: its heading takes folders dropped on it and its "+"
// the folder on screen, as One Commander's; its right click renames, moves
// or deletes it.
#[allow(clippy::too_many_arguments)]
fn bookmark_group(
    page: &mut ExplorerPane,
    list: Div,
    group: &BookmarkGroup,
    draw: Draw,
    first: &mut bool,
    icon_px: u32,
    cx: &mut Context<ExplorerPane>,
) -> Div {
    let id = group.id;
    let key = format!("bookmarks-{id}");
    let editing = |purpose: SidebarInputPurpose, page: &ExplorerPane| {
        page.sidebar.input.as_ref().filter(|field| field.purpose == purpose).map(|field| field.input.clone())
    };
    let (mut list, heading, open) = begin_group(list, &key, group.name.clone(), true, None, draw, first, cx);
    if let Some(input) = editing(SidebarInputPurpose::RenameBookmarkGroup(id), page) {
        list = list.child(field_row(input, 0));
    } else if let Some(heading) = heading {
        let dragged = GroupDrag { id, name: group.name.clone() };
        list = list.child(
            heading
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_bookmark_group_menu(id, window, cx);
                    }),
                )
                .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgb(theme::accent_subtle())).text_color(rgb(theme::accent())))
                .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                    cx.stop_propagation();
                    this.drop_on_bookmarks(id, paths.paths(), cx);
                }))
                // A place dropped on the name goes to the end of the group.
                .drag_over::<SidebarDrag>(|style, _, _, _| style.bg(rgb(theme::accent_subtle())).text_color(rgb(theme::accent())))
                .on_drop(cx.listener(move |this, drag: &SidebarDrag, _, cx| {
                    cx.stop_propagation();
                    this.drop_place_on_group(id, None, drag, cx);
                }))
                // The group itself, carried by its name to another's.
                .on_drag(dragged, |drag, _, _, cx| cx.new(|_| DragChip { name: drag.name.clone() }))
                .drag_over::<GroupDrag>(|style, _, _, _| style.border_t_2().border_color(rgb(theme::accent())))
                .on_drop(cx.listener(move |_, drag: &GroupDrag, _, cx| {
                    cx.stop_propagation();
                    Bookmarks::move_group_to(drag.id, id, cx);
                }))
                .child(dots_button(
                    SharedString::from(format!("bookmark-menu-{id}")),
                    cx,
                    move |this, window, cx| this.open_bookmark_group_menu(id, window, cx),
                ))
                .child(plus_button(
                    SharedString::from(format!("bookmark-add-{id}")),
                    t!("Adicionar a pasta atual"),
                    cx,
                    move |this, _, cx| this.bookmark_current_folder(id, cx),
                )),
        );
    }
    if !open {
        return list;
    }
    if group.items.is_empty() && !draw.icons_only {
        list = list.child(
            div()
                .px(px(8.0))
                .py(px(4.0))
                .text_xs()
                .text_color(rgb(theme::muted()))
                .child(t!("Arraste pastas ou lugares da barra para cá, ou use o +.")),
        );
    }
    for (index, bookmark) in group.items.iter().enumerate() {
        if let Some(input) = editing(SidebarInputPurpose::RenameBookmark { group: id, index }, page) {
            list = list.child(field_row(input, 0));
            continue;
        }
        let place = bookmark.place();
        let menu = RowMenu::Bookmark { group: id, index, path: bookmark.path.clone() };
        let prefix = format!("bookmark-{id}");
        let spec = RowSpec { prefix: &prefix, depth: 0, expandable: None, menu };
        list = list.child(
            row(page, &place, spec, draw, icon_px, cx)
                // Dropped on a row, a place goes where that row is, pushing
                // it down: the line over the row says so.
                .drag_over::<SidebarDrag>(|style, _, _, _| style.border_t_2().border_color(rgb(theme::accent())))
                .on_drop(cx.listener(move |this, drag: &SidebarDrag, _, cx| {
                    cx.stop_propagation();
                    this.drop_place_on_group(id, Some(index), drag, cx);
                })),
        );
    }
    list
}

// What a cell of the band opens.
#[derive(Clone)]
enum BandTarget {
    Place(Place),
    Folder(String),
}

// One cell of the band, already decided: what it shows and what it opens.
struct BandItem {
    id: SharedString,
    icon: AnyElement,
    name: String,
    active: bool,
    target: BandTarget,
    // How full a drive is, drawn under its name.
    space: Option<crate::explorer::drives::Space>,
    // Right click: the place's Windows menu, or a tag's menu.
    menu: Option<BandMenu>,
    // Shown after the name, muted: a tag's item count.
    note: Option<String>,
    // The cell is a field instead: the tag being named.
    editing: Option<Entity<gpui_component::input::InputState>>,
}

#[derive(Clone)]
enum BandMenu {
    Place(Place),
    Tag(u32),
    NetworkPlace(String),
    Recent(String),
    Bookmark { group: u32, index: usize, path: String },
}

const BAND_CELL_WIDTH: f32 = 176.0;
const BAND_ROW: f32 = 30.0;
const BAND_HEADER: f32 = 24.0;
const BAND_PADDING: f32 = 8.0;

// The sidebar docked above or below the listing. A wide, short space: the
// places run in columns, top to bottom and then to the next column, as
// Explorer's List view lays out files, each group under its name and apart
// from the next by a line. The wheel slides the band sideways. A taller band
// simply fits more rows in each column, so nothing wraps into a heap of
// chips. The groups and their order are the style's, as in the column.
fn render_band(
    page: &mut ExplorerPane,
    places: &Places,
    look: &SidebarLook,
    icon_px: u32,
    cx: &mut Context<ExplorerPane>,
) -> impl IntoElement + use<> {
    let height = super::panels::sidebar_band_height();
    let rows = (((height - BAND_PADDING * 2.0 - BAND_HEADER) / BAND_ROW).floor() as usize).max(1);
    let label = look.drive_label;

    let place_items = |page: &mut ExplorerPane, members: &[Place], group: &str, cx: &mut Context<ExplorerPane>| {
        members
            .iter()
            .map(|place| band_place(page, place, group, label, icon_px, cx))
            .collect::<Vec<_>>()
    };

    let mut sections: Vec<(String, &'static str, Vec<BandItem>, Option<AnyElement>)> = Vec::new();
    for section in &look.sections {
        match section {
            SidebarSection::QuickAccess => {
                let quick = place_items(page, &places.quick_access, "quick", cx);
                sections.push((t!("Acesso rápido").to_string(), "icons/star.svg", quick, None));
            }
            SidebarSection::Favorites => {
                let favorites = place_items(page, &places.favorites, "favorite", cx);
                sections.push((t!("Favoritos").to_string(), "icons/star.svg", favorites, None));
            }
            SidebarSection::UserFolders => {
                let folders = place_items(page, &places.user_folders, "user", cx);
                sections.push((t!("Pastas pessoais").to_string(), "icons/home.svg", folders, None));
            }
            SidebarSection::Recent => {
                let items = Bookmarks::recent(cx)
                    .into_iter()
                    .take(look.recent_count as usize)
                    .map(|path| {
                        let place = folder_place(&path, None);
                        let mut item = band_place(page, &place, "recent", label, icon_px, cx);
                        item.menu = Some(BandMenu::Recent(path));
                        item
                    })
                    .collect();
                sections.push((t!("Recentes").to_string(), "icons/clock.svg", items, None));
            }
            SidebarSection::Bookmarks => {
                for group in Bookmarks::groups(cx) {
                    let items = group
                        .items
                        .iter()
                        .enumerate()
                        .map(|(index, bookmark)| {
                            let place = folder_place(&bookmark.path, Some(bookmark.name()));
                            let mut item = band_place(page, &place, &format!("bookmark-{}", group.id), label, icon_px, cx);
                            item.menu = Some(BandMenu::Bookmark { group: group.id, index, path: bookmark.path.clone() });
                            item
                        })
                        .collect();
                    let id = group.id;
                    let add = plus_button(
                        SharedString::from(format!("band-bookmark-add-{id}")),
                        t!("Adicionar a pasta atual"),
                        cx,
                        move |this, _, cx| this.bookmark_current_folder(id, cx),
                    )
                    .into_any_element();
                    sections.push((group.name, "icons/star.svg", items, Some(add)));
                }
            }
            SidebarSection::Drives => {
                let drives = place_items(page, &places.drives, "drive", cx);
                sections.push((t!("Discos").to_string(), "icons/database.svg", drives, None));
            }
            SidebarSection::ThisPc => {
                let pc_name = places
                    .this_pc
                    .as_ref()
                    .map(|place| place.name.clone())
                    .unwrap_or_else(|| t!("Este Computador").to_string());
                let mut drives = Vec::new();
                if let Some(this_pc) = &places.this_pc {
                    drives.push(band_place(page, this_pc, "pc", label, icon_px, cx));
                }
                drives.extend(place_items(page, &places.drives, "pc-drive", cx));
                sections.push((pc_name, "icons/app-window.svg", drives, None));
            }
            SidebarSection::Network => {
                let mut other = place_items(page, &places.other, "other", cx);
                for address in crate::explorer::network::NetworkPlaces::all(cx) {
                    let active = crate::explorer::same_path(&address, &page.cwd);
                    other.push(BandItem {
                        id: SharedString::from(format!("band-network-{address}")),
                        icon: Icon::new(Icon::empty()).path("icons/server.svg").size_4().text_color(rgb(theme::muted())).into_any_element(),
                        name: crate::explorer::network::display_name(&address),
                        active,
                        target: BandTarget::Folder(address.clone()),
                        space: None,
                        menu: Some(BandMenu::NetworkPlace(address)),
                        note: None,
                        editing: None,
                    });
                }
                sections.push((t!("Locais").to_string(), "icons/folder.svg", other, None));
            }
            SidebarSection::Tags => {
                if !crate::explorer::tags::in_sidebar() {
                    continue;
                }
                let counts = Tags::counts(cx);
                let tags: Vec<BandItem> = Tags::all(cx)
                    .into_iter()
                    .map(|tag| {
                        let location = crate::explorer::tags::location(tag.id);
                        BandItem {
                            id: SharedString::from(format!("band-tag-{}", tag.id)),
                            icon: div().size(px(10.0)).mx(px(3.0)).rounded_full().bg(rgb(tag.color)).into_any_element(),
                            name: tag.name,
                            active: page.cwd == location,
                            target: BandTarget::Folder(location),
                            space: None,
                            menu: Some(BandMenu::Tag(tag.id)),
                            note: counts.get(&tag.id).filter(|count| **count > 0).map(|count| count.to_string()),
                            editing: page
                                .sidebar
                                .input
                                .as_ref()
                                .filter(|field| field.purpose == SidebarInputPurpose::RenameTag(tag.id))
                                .map(|field| field.input.clone()),
                        }
                    })
                    .collect();
                let new_tag = div()
                    .id("band-tag-new")
                    .size(px(18.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .text_color(rgb(theme::muted()))
                    .hover(|style| style.bg(rgb(theme::bg_hover())).text_color(rgb(theme::fg())))
                    .tooltip(|window, cx| gpui_component::tooltip::Tooltip::new(t!("Nova tag")).build(window, cx))
                    .on_click(cx.listener(|this, _, window, cx| {
                        let name = t!("Nova tag").to_string();
                        let id = Tags::create(name.clone(), &[], cx);
                        this.begin_sidebar_input(SidebarInputPurpose::RenameTag(id), name, window, cx);
                    }))
                    .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgb(theme::accent_subtle())).text_color(rgb(theme::accent())))
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                        cx.stop_propagation();
                        this.drop_on_new_tag(paths.paths(), window, cx);
                    }))
                    .child(Icon::new(IconName::Plus).size_3())
                    .into_any_element();
                sections.push((t!("Tags").to_string(), "icons/tag.svg", tags, Some(new_tag)));
            }
        }
    }

    // Not shrunk to the band: as wide as its columns, so the band has
    // something to scroll (in a block parent it took the band's width and
    // the wheel moved nothing).
    let mut band = div().h_full().flex_shrink_0().flex().items_start().px(px(6.0));
    let mut first = true;
    for (name, icon, items, extra) in sections {
        if items.is_empty() && extra.is_none() {
            continue;
        }
        if !first {
            band = band.child(div().w(px(1.0)).h_full().mx(px(6.0)).flex_shrink_0().bg(rgb(theme::border())));
        }
        first = false;
        let mut columns = div().flex().items_start();
        let mut items = items.into_iter().peekable();
        while items.peek().is_some() {
            let column: Vec<AnyElement> = items.by_ref().take(rows).map(|item| band_cell(item, cx)).collect();
            columns = columns.child(div().w(px(BAND_CELL_WIDTH)).flex_shrink_0().flex().flex_col().children(column));
        }
        band = band.child(
            div()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(BAND_HEADER))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::muted()))
                        .child(Icon::new(Icon::empty()).path(icon).size_3())
                        .child(div().whitespace_nowrap().child(name))
                        .children(extra),
                )
                .child(columns),
        );
    }
    div()
        .id("sidebar")
        .size_full()
        .when(!page.framed.sidebar, |this| this.bg(rgb(theme::bg())))
        .py(px(BAND_PADDING))
        .flex()
        .overflow_x_scroll()
        .child(band)
}

// A place of the Shell as a cell of the band.
fn band_place(
    page: &mut ExplorerPane,
    place: &Place,
    group: &str,
    label: DriveLabel,
    icon_px: u32,
    cx: &mut Context<ExplorerPane>,
) -> BandItem {
    let active = place_is_open(place, &page.cwd);
    let icon = match page.sidebar.icons.icon_in(&place.parsing_name, true, icon_px, |pane| &mut pane.sidebar.icons, cx) {
        Some(image) => img(ImageSource::Render(image)).size(px(ICON_SIZE)).flex_shrink_0().into_any_element(),
        None => Icon::new(IconName::Folder).size_4().text_color(rgb(theme::gray_500())).into_any_element(),
    };
    BandItem {
        id: SharedString::from(format!("band-{group}-{}", place.parsing_name)),
        icon,
        name: if is_drive(place) { drive_name(place, label) } else { place.name.clone() },
        active,
        target: BandTarget::Place(place.clone()),
        space: place.path.as_deref().and_then(|path| crate::explorer::drives::space(path, cx)),
        menu: Some(BandMenu::Place(place.clone())),
        note: None,
        editing: None,
    }
}

fn band_cell(item: BandItem, cx: &mut Context<ExplorerPane>) -> AnyElement {
    if let Some(input) = item.editing {
        return div()
            .h(px(BAND_ROW))
            .mx(px(2.0))
            .flex()
            .items_center()
            .child(gpui_component::input::Input::new(&input).small())
            .into_any_element();
    }
    let opened = item.target.clone();
    let in_tab = item.target.clone();
    let menu = item.menu.clone();
    // A tag takes the files dropped on it.
    let dropped_tag = match &item.menu {
        Some(BandMenu::Tag(tag)) => Some(*tag),
        _ => None,
    };
    let tip = item.name.clone();
    let name = div()
        .min_w(px(0.0))
        .text_sm()
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .text_color(rgb(theme::fg()))
        .child(item.name);
    div()
        .id(item.id)
        .h(px(BAND_ROW))
        .mx(px(2.0))
        .px(px(8.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .rounded(px(6.0))
        .cursor_pointer()
        .when(item.active, |this| this.bg(rgb(theme::accent_subtle())))
        .when(!item.active, |this| this.hover(|this| this.bg(rgb(theme::bg_hover()))))
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
        .on_click(cx.listener(move |this, _, window, cx| match &opened {
            BandTarget::Place(place) => this.open_place(place, window, cx),
            BandTarget::Folder(folder) => this.change_dir(folder.clone(), window, cx),
        }))
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                match &in_tab {
                    BandTarget::Place(place) => {
                        if let Some(address) = ExplorerPane::place_address(place) {
                            this.open_path_in_new_tab(&address, true, true, cx);
                        }
                    }
                    BandTarget::Folder(folder) => this.open_path_in_new_tab(folder, true, true, cx),
                }
            }),
        )
        .when_some(dropped_tag, |this, tag| {
            this.drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgb(theme::accent_subtle())))
                .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                    cx.stop_propagation();
                    this.drop_on_tag(tag, paths.paths(), cx);
                }))
        })
        .when_some(menu, |this, menu| {
            this.on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    match &menu {
                        BandMenu::Place(place) => this.open_place_menu(place.clone(), window, cx),
                        BandMenu::Tag(tag) => this.open_tag_menu(*tag, window, cx),
                        BandMenu::NetworkPlace(address) => this.open_network_place_menu(address.clone(), window, cx),
                        BandMenu::Recent(path) => this.open_recent_menu(path.clone(), window, cx),
                        BandMenu::Bookmark { group, index, path } => {
                            this.open_bookmark_menu(*group, *index, path.clone(), window, cx)
                        }
                    }
                }),
            )
        })
        .child(div().flex_shrink_0().child(item.icon))
        .child(div().flex_1().min_w(px(0.0)).flex().child(name))
        // A drive: how full it is, under its name, as This PC's cards. Placed
        // on the cell rather than in a column with the name: text in a flex
        // column is measured at its narrowest and came out as "…".
        .when_some(item.space, |this, space| {
            this.relative().pb(px(6.0)).child(
                div()
                    .absolute()
                    .left(px(32.0))
                    .right(px(10.0))
                    .bottom(px(3.0))
                    .child(crate::explorer::view::listing::usage_bar(Some(space), 3.0)),
            )
        })
        .when_some(item.note, |this, note| {
            this.child(div().flex_shrink_0().text_xs().text_color(rgb(theme::muted())).child(note))
        })
        .into_any_element()
}

// The places added by hand, then the row (or field) to add another.
fn network_rows(page: &mut ExplorerPane, mut list: Div, draw: Draw, cx: &mut Context<ExplorerPane>) -> Div {
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
                draw,
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
                draw,
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
fn tag_rows(page: &mut ExplorerPane, list: Div, draw: Draw, first: &mut bool, cx: &mut Context<ExplorerPane>) -> Div {
    let tags = Tags::all(cx);
    let counts = Tags::counts(cx);
    let (mut list, heading, open) =
        begin_group(list, "tags", t!("Tags").to_string(), true, Some(SidebarSection::Tags), draw, first, cx);
    if let Some(heading) = heading {
        list = list.child(
            heading
                // Files dropped on the heading (or its +) get a new tag of
                // their own, named at once.
                .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgb(theme::accent_subtle())).text_color(rgb(theme::accent())))
                .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                    cx.stop_propagation();
                    this.drop_on_new_tag(paths.paths(), window, cx);
                }))
                .child(plus_button("tag-new", t!("Nova tag"), cx, |this, window, cx| {
                    let name = t!("Nova tag").to_string();
                    let id = Tags::create(name.clone(), &[], cx);
                    this.begin_sidebar_input(SidebarInputPurpose::RenameTag(id), name, window, cx);
                })),
        );
    }
    if !open {
        return list;
    }
    if tags.is_empty() && !draw.icons_only {
        list = list.child(
            div()
                .px(px(8.0))
                .py(px(4.0))
                .text_xs()
                .text_color(rgb(theme::muted()))
                .child(t!("Arraste arquivos ou pastas para cá para criar uma tag.")),
        );
    }
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
        let was_active = active;
        let held_name = tag.name.clone();
        let count = counts.get(&tag.id).copied().unwrap_or(0);
        let id = tag.id;
        let tag_color = tag.color;
        let tag_name = tag.name.clone();
        list = list.child(
            simple_row(
                SharedString::from(format!("tag-{}", tag.id)),
                div().size(px(10.0)).mx(px(3.0)).rounded_full().bg(rgb(tag.color)).into_any_element(),
                tag.name.clone(),
                active,
                0,
                draw,
            )
            .when(count > 0 && !draw.icons_only, |this| {
                this.child(div().flex_shrink_0().text_xs().text_color(rgb(theme::muted())).child(count.to_string()))
            })
            // Three ways to name it, as users asked (Senkozinho and the owner,
            // 07/11/2026; the four tags that come with the app were only
            // renamed from their menu): holding the button down on it, three
            // clicks, or a second, slow click on the tag already open, as on
            // a file in the listing.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                    this.arm_tag_hold(id, held_name.clone(), window, cx);
                }),
            )
            // Let go off the tag before the hold was long enough: no rename.
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _window, _cx| this.slow_rename = None),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                if event.click_count() >= 3 {
                    this.slow_rename = None;
                    this.begin_sidebar_input(SidebarInputPurpose::RenameTag(id), tag_name.clone(), window, cx);
                    return;
                }
                this.arm_tag_rename(id, tag_name.clone(), was_active, event, window, cx);
                this.change_dir(location.clone(), window, cx);
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_tag_menu(id, window, cx);
                }),
            )
            // Files and folders dropped on a tag get it, as on Finder's and
            // the Files app's sidebar.
            .drag_over::<ExternalPaths>(move |style, _, _, _| {
                style.bg(rgb(theme::accent_subtle())).border_1().border_color(rgb(tag_color))
            })
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                cx.stop_propagation();
                this.drop_on_tag(id, paths.paths(), cx);
            })),
        );
    }
    list
}

impl ExplorerPane {
    /// A place dropped on group `group`, at row `at` or at the end: moved
    /// there when it came from a group, put there when from elsewhere.
    fn drop_place_on_group(&mut self, group: u32, at: Option<usize>, drag: &SidebarDrag, cx: &mut Context<Self>) {
        match drag.from {
            Some((from_group, from_index)) => {
                let end = Bookmarks::group(group, cx).map(|group| group.items.len()).unwrap_or(0);
                Bookmarks::move_item_to(from_group, from_index, group, at.unwrap_or(end), cx);
            }
            None => {
                if !Bookmarks::add_place(group, &drag.place, at, cx) {
                    let name = Bookmarks::group(group, cx).map(|group| group.name).unwrap_or_default();
                    self.set_status(
                        crate::explorer::types::StatusLevel::Info,
                        prime_files_core::tf!("Já está em {group}", group = name),
                    );
                }
            }
        }
        cx.notify();
    }

    /// A new group of bookmarks, its name field open; with `place` already
    /// in it. The Bookmarks section is turned on if the style left it out,
    /// or the group would not show.
    pub(crate) fn new_bookmark_group(&mut self, place: Option<Place>, window: &mut Window, cx: &mut Context<Self>) {
        let name = t!("Novo grupo").to_string();
        let id = Bookmarks::create_group(name.clone(), cx);
        if let Some(place) = place {
            Bookmarks::add_place(id, &place, None, cx);
        }
        let mut sections = crate::explorer::places::look().sections;
        if !sections.contains(&SidebarSection::Bookmarks) {
            sections.push(SidebarSection::Bookmarks);
            self.write_sidebar_sections(&sections, cx);
        }
        self.begin_sidebar_input(SidebarInputPurpose::RenameBookmarkGroup(id), name, window, cx);
    }

    /// Bookmarks the folders dropped on group `id`, saying so in the footer.
    fn drop_on_bookmarks(&mut self, id: u32, paths: &[std::path::PathBuf], cx: &mut Context<Self>) {
        let paths: Vec<String> = paths.iter().map(|path| path.to_string_lossy().into_owned()).collect();
        if paths.is_empty() {
            return;
        }
        let added = Bookmarks::add(id, &paths, cx);
        self.say_bookmarked(id, added, cx);
    }

    /// The "+" of group `id`: bookmarks the folder on screen.
    fn bookmark_current_folder(&mut self, id: u32, cx: &mut Context<Self>) {
        let added = Bookmarks::add(id, std::slice::from_ref(&self.cwd), cx);
        self.say_bookmarked(id, added, cx);
    }

    fn say_bookmarked(&mut self, id: u32, added: usize, cx: &mut Context<Self>) {
        let group = Bookmarks::group(id, cx).map(|group| group.name).unwrap_or_default();
        let (level, text) = if added == 0 {
            (
                crate::explorer::types::StatusLevel::Info,
                prime_files_core::tf!("Nada novo em {group}: só pastas que ainda não estão lá entram", group = group),
            )
        } else {
            (
                crate::explorer::types::StatusLevel::Info,
                prime_files_core::i18n::plural(added, prime_files_core::k!("{n} pasta adicionada a {group}"), prime_files_core::k!("{n} pastas adicionadas a {group}"))
                    .replace("{group}", &group),
            )
        };
        self.set_status(level, text);
        cx.notify();
    }

    // The slow click on a tag: one plain click on the tag already open, not
    // followed by a second, opens its name field once the double-click time
    // has passed. Any other click, or leaving the tag, calls it off.
    fn arm_tag_rename(
        &mut self,
        id: u32,
        name: String,
        was_active: bool,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.slow_rename = None;
        let plain = !(event.modifiers().shift || event.modifiers().control || event.modifiers().platform);
        if !was_active || event.click_count() != 1 || !plain || self.sidebar.input.is_some() {
            return;
        }
        let location = crate::explorer::tags::location(id);
        let delay = crate::explorer::file_ops::double_click_time();
        self.slow_rename = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update_in(cx, |pane, window, cx| {
                pane.slow_rename = None;
                if pane.cwd == location && pane.sidebar.input.is_none() {
                    pane.begin_sidebar_input(SidebarInputPurpose::RenameTag(id), name, window, cx);
                }
            })
            .ok();
        }));
    }

    // Holding the button down on a tag: its name field opens once the press
    // has lasted, unless the release (the click, which replaces this task)
    // comes first.
    fn arm_tag_hold(&mut self, id: u32, name: String, window: &mut Window, cx: &mut Context<Self>) {
        const HOLD: std::time::Duration = std::time::Duration::from_millis(600);
        self.slow_rename = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(HOLD).await;
            this.update_in(cx, |pane, window, cx| {
                pane.slow_rename = None;
                if pane.sidebar.input.is_none() {
                    pane.begin_sidebar_input(SidebarInputPurpose::RenameTag(id), name, window, cx);
                }
            })
            .ok();
        }));
    }

    /// Puts tag `id` on what was dropped on it, saying so in the footer.
    fn drop_on_tag(&mut self, id: u32, paths: &[std::path::PathBuf], cx: &mut Context<Self>) {
        let paths: Vec<String> = paths.iter().map(|path| path.to_string_lossy().into_owned()).collect();
        if paths.is_empty() {
            return;
        }
        let added = Tags::add(&paths, id, cx);
        let name = Tags::get(id, cx).map(|tag| tag.name).unwrap_or_default();
        let text = if added == 0 {
            prime_files_core::tf!("Já tinham a tag {tag}", tag = name)
        } else {
            prime_files_core::i18n::plural(added, prime_files_core::k!("{n} item marcado com {tag}"), prime_files_core::k!("{n} itens marcados com {tag}"))
                .replace("{tag}", &name)
        };
        self.set_status(crate::explorer::types::StatusLevel::Info, text);
        cx.notify();
    }

    /// A new tag on what was dropped, its name field open.
    fn drop_on_new_tag(&mut self, paths: &[std::path::PathBuf], window: &mut Window, cx: &mut Context<Self>) {
        let paths: Vec<String> = paths.iter().map(|path| path.to_string_lossy().into_owned()).collect();
        if paths.is_empty() {
            return;
        }
        let name = t!("Nova tag").to_string();
        let id = Tags::create(name.clone(), &paths, cx);
        self.begin_sidebar_input(SidebarInputPurpose::RenameTag(id), name, window, cx);
    }
}

// A sidebar row with no tree around it: an icon and a name. In a strip of
// icons, the icon alone, the name in its tooltip.
fn simple_row(
    id: impl Into<ElementId>,
    icon: AnyElement,
    name: String,
    active: bool,
    depth: usize,
    draw: Draw,
) -> Stateful<Div> {
    let tip = name.clone();
    div()
        .id(id)
        .w_full()
        .h(px(draw.row_height))
        .flex()
        .items_center()
        .gap(px(6.0))
        .when(draw.icons_only, |this| {
            this.justify_center()
                .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
        })
        .when(!draw.icons_only, |this| {
            this.pl(px(2.0 + draw.leaf_indent() + if draw.tree { depth as f32 * CHEVRON_WIDTH } else { 0.0 }))
                .pr(px(8.0))
        })
        .rounded(px(4.0))
        .cursor_pointer()
        .when(active, |this| this.bg(rgb(theme::accent_subtle())))
        .when(!active, |this| this.hover(|this| this.bg(rgb(theme::bg_hover()))))
        .child(div().flex_shrink_0().child(icon))
        .when(!draw.icons_only, |this| {
            this.child(
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
        })
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

// A node and, when open, everything under it. Outside a tree, the node is a
// link like any.
#[allow(clippy::too_many_arguments)]
fn tree(
    page: &mut ExplorerPane,
    mut list: Div,
    place: &Place,
    prefix: &str,
    depth: usize,
    draw: Draw,
    icon_px: u32,
    cx: &mut Context<ExplorerPane>,
) -> Div {
    if !draw.tree {
        let spec = RowSpec { prefix, depth, expandable: None, menu: RowMenu::Place };
        return list.child(row(page, place, spec, draw, icon_px, cx));
    }
    let open = page.sidebar.expanded.contains(&place.parsing_name);
    let spec = RowSpec { prefix, depth, expandable: Some(open), menu: RowMenu::Place };
    list = list.child(row(page, place, spec, draw, icon_px, cx));
    if open
        && let Some(path) = &place.path
        && let Some(children) = page.sidebar.children.get(path).cloned()
    {
        for child in &children {
            list = tree(page, list, child, prefix, depth + 1, draw, icon_px, cx);
        }
    }
    list
}

// What a row's right click opens.
#[derive(Clone)]
enum RowMenu {
    /// The place's Windows menu.
    Place,
    /// A recent folder's: open in a new tab, forget it, clear the history.
    Recent(String),
    /// A bookmark's: rename, move, remove.
    Bookmark { group: u32, index: usize, path: String },
}

struct RowSpec<'a> {
    // Keeps the ids apart when one folder is in two groups (Desktop in Quick
    // access and in the user's folders): rows with one id share their
    // hover and their clicks.
    prefix: &'a str,
    depth: usize,
    // `Some(open)` for a tree node, `None` for a leaf (Quick access entries
    // are links, as in Explorer).
    expandable: Option<bool>,
    menu: RowMenu,
}

fn row(
    page: &mut ExplorerPane,
    place: &Place,
    spec: RowSpec,
    draw: Draw,
    icon_px: u32,
    cx: &mut Context<ExplorerPane>,
) -> Stateful<Div> {
    let RowSpec { prefix, depth, expandable, menu } = spec;
    let active = place_is_open(place, &page.cwd);
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
    let name = if is_drive(place) { drive_name(place, draw.drive_label) } else { place.name.clone() };
    let id = SharedString::from(format!("place-{prefix}-{depth}-{}", place.parsing_name));
    let opened = place.clone();
    let shown = place.clone();
    let drive_space = match draw.drive_usage {
        DriveUsage::None => None,
        DriveUsage::Beside | DriveUsage::Below => place.path.as_deref().and_then(|path| crate::explorer::drives::space(path, cx)),
    };
    let below = draw.drive_usage == DriveUsage::Below && drive_space.is_some();
    let dragged = SidebarDrag {
        place: Place { name: name.clone(), ..place.clone() },
        from: match &menu {
            RowMenu::Bookmark { group, index, .. } => Some((*group, *index)),
            RowMenu::Place | RowMenu::Recent(_) => None,
        },
    };
    let base = div()
        .id(id)
        .w_full()
        .h(px(draw.row_height + if below { BAR_BELOW } else { 0.0 }))
        .relative()
        .flex()
        .items_center()
        .gap(px(6.0))
        .rounded(px(4.0))
        .cursor_pointer()
        .when(active, |this| this.bg(rgb(theme::accent_subtle())))
        .when(!active, |this| this.hover(|this| this.bg(rgb(theme::bg_hover()))))
        .on_click(cx.listener(move |this, _, window, cx| this.open_place(&opened, window, cx)))
        // Any place can be carried to a group of bookmarks.
        .on_drag(dragged, |drag, _, _, cx| cx.new(|_| DragChip { name: drag.place.name.clone() }))
        .when_some(ExplorerPane::place_address(place), |this, path| {
            this.on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_path_in_new_tab(&path, true, true, cx);
                }),
            )
        })
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                match &menu {
                    RowMenu::Place => this.open_place_menu(shown.clone(), window, cx),
                    RowMenu::Recent(path) => this.open_recent_menu(path.clone(), window, cx),
                    RowMenu::Bookmark { group, index, path } => {
                        this.open_bookmark_menu(*group, *index, path.clone(), window, cx)
                    }
                }
            }),
        );
    if draw.icons_only {
        let tip = name;
        return base
            .justify_center()
            .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
            .child(icon);
    }
    let chevron = match expandable {
        Some(open) => {
            let toggled = place.clone();
            div()
                .id(SharedString::from(format!("toggle-{prefix}-{}", place.parsing_name)))
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
        None => div().w(px(draw.leaf_indent())).flex_shrink_0().into_any_element(),
    };
    // Directory Opus's "Lines": a guide down each level above this one,
    // under the chevron of the node it hangs from.
    let guides: Vec<AnyElement> = if draw.tree_lines && draw.tree {
        (0..depth)
            .map(|level| {
                div()
                    .absolute()
                    .top(px(-1.0))
                    .bottom(px(0.0))
                    .left(px(2.0 + level as f32 * CHEVRON_WIDTH + CHEVRON_WIDTH / 2.0))
                    .w(px(1.0))
                    .bg(rgb(theme::border()))
                    .into_any_element()
            })
            .collect()
    } else {
        Vec::new()
    };
    let indent = if draw.tree { depth as f32 * CHEVRON_WIDTH } else { 0.0 };
    base.pl(px(2.0 + indent))
        .pr(px(8.0))
        .children(guides)
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
                .child(name),
        )
        .when_some(drive_space.filter(|_| !below), |this, space| {
            // How full the drive is, beside its name: a glance at C: and D:
            // without opening This PC.
            this.child(
                div()
                    .id(SharedString::from(format!("drive-space-{prefix}-{}", place.parsing_name)))
                    .w(px(34.0))
                    .flex_shrink_0()
                    .child(crate::explorer::view::listing::usage_bar(Some(space), 4.0))
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(space.describe()).build(window, cx)
                    }),
            )
        })
        // One Commander's: how much is free and how big the drive is,
        // after the name, over the bar.
        .when_some(drive_space.filter(|_| below), |this, space| {
            use prime_files_ui::components::file_list::human_bytes;
            this.child(
                div()
                    .flex_shrink_0()
                    .text_xs()
                    .text_color(rgb(theme::muted()))
                    .child(format!("{} / {}", human_bytes(space.free), human_bytes(space.total))),
            )
        })
        // File Pilot's: the bar the width of the name, under it. Placed on
        // the row rather than in a column with the name: text in a flex
        // column is measured at its narrowest and came out as "…".
        .when_some(drive_space.filter(|_| below), |this, space| {
            this.pb(px(BAR_BELOW)).child(
                div()
                    .id(SharedString::from(format!("drive-space-{prefix}-{}", place.parsing_name)))
                    .absolute()
                    .left(px(2.0 + indent + draw.leaf_indent() + ICON_SIZE + 6.0))
                    .right(px(10.0))
                    .bottom(px(4.0))
                    .child(crate::explorer::view::listing::usage_bar(Some(space), 3.0))
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

#[cfg(test)]
mod tests {
    // Not `super::*`: it brings gpui's `test` attribute over the standard one.
    use super::{DriveLabel, Place, drive_name, folder_place, is_drive};

    fn drive(name: &str) -> Place {
        Place { name: name.to_string(), path: Some(r"D:\".to_string()), parsing_name: r"D:\".to_string(), pinned: false }
    }

    #[test]
    fn drive_names_follow_the_label_setting() {
        let local = drive("Disco Local (D:)");
        assert!(is_drive(&local));
        assert_eq!(drive_name(&local, DriveLabel::Name), "Disco Local (D:)");
        assert_eq!(drive_name(&local, DriveLabel::LetterFirst), "D: Disco Local");
        assert_eq!(drive_name(&local, DriveLabel::Letter), "D:");
        // A name without the letter in brackets keeps its own words.
        let odd = drive("Backup");
        assert_eq!(drive_name(&odd, DriveLabel::LetterFirst), "Backup");
        assert_eq!(drive_name(&odd, DriveLabel::Letter), "D:");
        assert!(!is_drive(&folder_place(r"D:\Projetos", None)));
    }
}

// Whether `place` is the location on screen. This PC, the Recycle Bin and
// Network have no path, only their shell address, which is what the pane's
// folder is while they are open (issue #45).
fn place_is_open(place: &Place, cwd: &str) -> bool {
    match place.path.as_deref() {
        Some(path) => crate::explorer::same_path(path, cwd),
        None => place.parsing_name.eq_ignore_ascii_case(cwd),
    }
}
