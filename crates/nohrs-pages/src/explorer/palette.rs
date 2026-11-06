//! Ctrl+T: a search box over the window, as Zen Browser's new tab is. It
//! lists the open tabs that match (to go to one instead of opening it twice)
//! and every file and folder of the computer whose name matches, from the
//! whole-computer index. Enter or a click opens the hit in a new tab — a
//! folder shows itself, a file its folder with it selected — and a hit can be
//! dragged out like a file of the listing. With nothing typed, Enter opens a
//! plain new tab, which is what Ctrl+T did before.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::prelude::*;
use gpui::*;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Icon, IconName};
use nohrs_core::t;
use nohrs_core::telemetry::LogErr;
use nohrs_ui::theme::theme;

use super::page::ExplorerPage;

// How many hits of the index the box shows.
const HITS: usize = 40;
const ROW: f32 = 46.0;
const VISIBLE_ROWS: usize = 9;
// How far the pointer moves with the button down before a press is a drag.
const DRAG_THRESHOLD: f32 = 6.0;

/// The box while it is open.
pub(crate) struct Palette {
    input: Entity<InputState>,
    _subscription: Subscription,
    query: String,
    rows: Vec<Row>,
    selected: usize,
    cancel: Arc<AtomicBool>,
    _search: Option<Task<()>>,
    // The row pressed and where, until it is released or becomes a drag.
    press: Option<(usize, Point<Pixels>)>,
    scroll: ScrollHandle,
    // The index generation the hits came from.
    generation: u64,
}

#[derive(Clone)]
enum Row {
    /// An open tab: its entity id, title and folder.
    Tab { id: u64, title: String, folder: String },
    /// A path typed in full that exists.
    Typed { path: String, dir: bool },
    /// A hit of the index.
    Hit { name: String, path: String, dir: bool },
}

impl Row {
    fn path(&self) -> Option<(&str, bool)> {
        match self {
            Row::Tab { .. } => None,
            Row::Typed { path, dir } | Row::Hit { path, dir, .. } => Some((path, *dir)),
        }
    }
}

impl ExplorerPage {
    /// Opens the box, or closes it when it is already open.
    pub(crate) fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
            return;
        }
        // The index loads (or is brought up to date) while the first letters
        // are typed.
        super::everything::ensure(cx);
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("Buscar arquivos e pastas no computador ou digitar um caminho"))
        });
        input.update(cx, |state, cx| state.focus(window, cx));
        let subscription = cx.subscribe_in(&input, window, |page, _input, event: &InputEvent, window, cx| match event {
            InputEvent::Change => page.palette_changed(cx),
            InputEvent::PressEnter { .. } => page.activate_palette_row(None, window, cx),
            _ => {}
        });
        self.palette = Some(Palette {
            input,
            _subscription: subscription,
            query: String::new(),
            rows: Vec::new(),
            selected: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            _search: None,
            press: None,
            scroll: ScrollHandle::new(),
            generation: cx.try_global::<super::everything::Everything>().map_or(0, |state| state.generation),
        });
        self.palette_changed(cx);
    }

    pub(crate) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(palette) = self.palette.take() {
            palette.cancel.store(true, Ordering::Relaxed);
            self.group.focus_active(window, cx);
            cx.notify();
        }
    }

    // The text changed: the tabs are matched here, the index off the UI
    // thread, and its hits replace the old ones when they come.
    fn palette_changed(&mut self, cx: &mut Context<Self>) {
        let tabs = self.tabs(cx);
        let titles: Vec<(u64, String, String)> = tabs
            .into_iter()
            .filter_map(|(id, folder, _)| {
                let tab = self.group.all_tabs().into_iter().find(|tab| tab.entity_id().as_u64() == id)?;
                let title = crate::pane_group::PaneItem::tab_title(tab.read(cx), cx);
                Some((id, title, folder))
            })
            .collect();
        let cwd = self.group.active_pane().read(cx).cwd.clone();
        let Some(palette) = self.palette.as_mut() else {
            return;
        };
        let query = palette.input.read(cx).value().to_string();
        palette.query = query.clone();
        let needle = query.trim().to_lowercase();

        let mut rows: Vec<Row> = titles
            .into_iter()
            .filter(|(_, title, folder)| {
                needle.is_empty() || title.to_lowercase().contains(&needle) || folder.to_lowercase().contains(&needle)
            })
            .map(|(id, title, folder)| Row::Tab { id, title, folder })
            .collect();
        let typed = query.trim().trim_matches('"');
        if Path::new(typed).is_absolute() && Path::new(typed).exists() {
            rows.insert(0, Row::Typed { path: typed.to_string(), dir: Path::new(typed).is_dir() });
        }
        // The hits of the last search stay until the new ones come, so the
        // list does not blink at each letter.
        rows.extend(palette.rows.iter().filter(|row| matches!(row, Row::Hit { .. })).cloned());
        palette.rows = rows;
        palette.selected = 0;

        palette.cancel.store(true, Ordering::Relaxed);
        if needle.is_empty() {
            palette.rows.retain(|row| !matches!(row, Row::Hit { .. }));
            cx.notify();
            return;
        }
        let Some(finder) = super::everything::finder(cx, &query, &cwd) else {
            cx.notify();
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        palette.cancel = cancel.clone();
        let search = cx.background_spawn(async move {
            let found = finder.run(&cancel);
            // Names gone since the index was made are left out.
            found
                .into_iter()
                .filter(|(_, path, _)| !cancel.load(Ordering::Relaxed) && Path::new(path).exists())
                .take(HITS)
                .collect::<Vec<_>>()
        });
        palette._search = Some(cx.spawn(async move |page: WeakEntity<Self>, cx| {
            let found = search.await;
            page.update(cx, |page, cx| {
                let Some(palette) = page.palette.as_mut() else {
                    return;
                };
                if palette.query != query {
                    return;
                }
                palette.rows.retain(|row| !matches!(row, Row::Hit { .. }));
                palette
                    .rows
                    .extend(found.into_iter().map(|(name, path, dir)| Row::Hit { name, path, dir }));
                palette.selected = palette.selected.min(palette.rows.len().saturating_sub(1));
                cx.notify();
            })
            .log_err();
        }));
        cx.notify();
    }

    /// The index was loaded, rebuilt or grew: the words typed are searched
    /// again, at most once per index generation.
    pub(crate) fn palette_index_changed(&mut self, cx: &mut Context<Self>) {
        let generation = cx.try_global::<super::everything::Everything>().map_or(0, |state| state.generation);
        let Some(palette) = self.palette.as_mut() else {
            return;
        };
        if palette.generation == generation {
            cx.notify();
            return;
        }
        palette.generation = generation;
        self.palette_changed(cx);
    }

    fn move_palette_selection(&mut self, step: isize, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.as_mut() else {
            return;
        };
        if palette.rows.is_empty() {
            return;
        }
        let last = palette.rows.len() as isize - 1;
        palette.selected = (palette.selected as isize + step).clamp(0, last) as usize;
        palette.scroll.scroll_to_item(palette.selected);
        cx.notify();
    }

    // Enter (`None`: the selected row) or a click on `row`.
    fn activate_palette_row(&mut self, row: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.as_ref() else {
            return;
        };
        let chosen = palette.rows.get(row.unwrap_or(palette.selected)).cloned();
        let empty = palette.query.trim().is_empty();
        self.close_palette(window, cx);
        match chosen {
            Some(Row::Tab { id, .. }) => {
                if let Some(tab) = self.group.all_tabs().into_iter().find(|tab| tab.entity_id().as_u64() == id)
                    && let Some((pane, index)) = self.group.locate_tab(tab.entity_id())
                {
                    self.group.set_active_tab(pane, index, window, cx);
                    self.group.set_active(pane, window, cx);
                    self.group.focus_active(window, cx);
                    cx.notify();
                }
            }
            Some(Row::Typed { path, dir }) | Some(Row::Hit { path, dir, .. }) => {
                let (folder, select) = folder_and_selection(&path, dir);
                self.open_in_new_tab(folder, select, false, window, cx);
            }
            None if empty => {
                let pane = self.group.active();
                self.new_tab_in_pane(pane, window, cx);
            }
            None => {}
        }
    }

    // A press that moved far enough with the button down: the hit leaves the
    // box as a file dragged from the listing would.
    fn palette_drag_moved(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.as_mut() else {
            return;
        };
        let Some((row, start)) = palette.press else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            palette.press = None;
            return;
        }
        let moved = event.position - start;
        if f32::from(moved.x).hypot(f32::from(moved.y)) < DRAG_THRESHOLD {
            return;
        }
        palette.press = None;
        let Some((path, _)) = palette.rows.get(row).and_then(Row::path) else {
            return;
        };
        let path = PathBuf::from(path);
        let Some(hwnd) = crate::ipc::window_hwnd(window) else {
            return;
        };
        // Out of the way first, so a drop on this window lands on the listing.
        self.close_palette(window, cx);
        // The drag runs its own message loop until the drop: from a task, so
        // the app is not borrowed meanwhile (as in `drag.rs`).
        cx.spawn_in(window, async move |_page, _cx| {
            if let Err(error) = super::drag::drag_out(hwnd, &[path]) {
                tracing::warn!("could not drag from the search box: {error}");
            }
        })
        .detach();
    }

    /// The box over the window, when it is open.
    pub(crate) fn render_palette(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let palette = self.palette.as_ref()?;
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        let width = (vw - 48.0).clamp(280.0, 680.0);
        let left = (vw - width) / 2.0;
        let top = (vh * 0.12).clamp(24.0, 110.0);

        let rows: Vec<AnyElement> = palette
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| render_row(row, index, index == palette.selected, cx))
            .collect();
        let empty = rows.is_empty();
        let searching = !palette.query.trim().is_empty() && palette.rows.iter().all(|row| !matches!(row, Row::Hit { .. }));
        // A first indexing still under way says how far it got instead.
        let indexing = cx
            .try_global::<super::everything::Everything>()
            .and_then(|state| state.building.as_ref())
            .map(|found| found.load(Ordering::Relaxed));
        let list_height = (palette.rows.len().min(VISIBLE_ROWS) as f32) * ROW;

        let card = div()
            .id("palette")
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .flex()
            .flex_col()
            .rounded(px(10.0))
            .border_1()
            .border_color(rgb(theme::border()))
            .bg(rgb(theme::bg_secondary()))
            .shadow_xl()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(|page, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "down" => page.move_palette_selection(1, cx),
                    "up" => page.move_palette_selection(-1, cx),
                    "pagedown" => page.move_palette_selection(VISIBLE_ROWS as isize, cx),
                    "pageup" => page.move_palette_selection(-(VISIBLE_ROWS as isize), cx),
                    "escape" => page.close_palette(window, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .on_mouse_move(cx.listener(|page, event: &MouseMoveEvent, window, cx| {
                page.palette_drag_moved(event, window, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|page, _: &MouseUpEvent, _, _| {
                    if let Some(palette) = page.palette.as_mut() {
                        palette.press = None;
                    }
                }),
            )
            .child(
                div()
                    .h(px(52.0))
                    .px(px(14.0))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .child(Icon::new(IconName::Search).size_4().text_color(rgb(theme::muted())))
                    .child(div().flex_1().min_w(px(0.0)).child(Input::new(&palette.input).appearance(false))),
            )
            .when(!empty, |card| {
                card.child(div().h(px(1.0)).mx(px(10.0)).bg(rgb(theme::border()))).child(
                    div()
                        .id("palette-rows")
                        .h(px(list_height + 8.0))
                        .p(px(4.0))
                        .overflow_y_scroll()
                        .track_scroll(&palette.scroll)
                        .children(rows),
                )
            })
            .when(searching, |card| {
                card.child(
                    div()
                        .px(px(16.0))
                        .py(px(8.0))
                        .text_xs()
                        .text_color(rgb(theme::muted()))
                        .child(match indexing {
                            Some(found) => nohrs_core::tf!("Indexando o computador… {n} itens", n = found),
                            None => t!("Buscando no computador…").to_string(),
                        }),
                )
            })
            .child(
                div()
                    .px(px(16.0))
                    .py(px(8.0))
                    .border_t_1()
                    .border_color(rgb(theme::border()))
                    .text_xs()
                    .text_color(rgb(theme::muted()))
                    .child(t!("Enter abre numa nova aba · arraste para levar o arquivo · Esc fecha")),
            );

        Some(
            deferred(
                anchored().position_mode(AnchoredPositionMode::Window).position(point(px(0.0), px(0.0))).child(
                    div()
                        .id("palette-backdrop")
                        .w(px(vw))
                        .h(px(vh))
                        .bg(rgba(0x0000_0040))
                        .occlude()
                        .on_mouse_down(MouseButton::Left, cx.listener(|page, _, window, cx| page.close_palette(window, cx)))
                        .child(card),
                ),
            )
            .with_priority(20)
            .into_any_element(),
        )
    }
}

// The folder a new tab opens on, and what it selects there.
fn folder_and_selection(path: &str, dir: bool) -> (String, Option<String>) {
    if dir {
        return (path.to_string(), None);
    }
    match Path::new(path).parent() {
        Some(parent) => (parent.to_string_lossy().into_owned(), Some(path.to_string())),
        None => (path.to_string(), None),
    }
}

fn render_row(row: &Row, index: usize, selected: bool, cx: &mut Context<ExplorerPage>) -> AnyElement {
    let (icon, name, detail, hint): (&'static str, String, String, &'static str) = match row {
        Row::Tab { title, folder, .. } => ("icons/app-window.svg", title.clone(), folder.clone(), t!("Ir para a aba")),
        Row::Typed { path, dir } => (
            if *dir { "icons/folder-open.svg" } else { "icons/file.svg" },
            path.clone(),
            String::new(),
            t!("Abrir em nova aba"),
        ),
        Row::Hit { name, path, dir } => (
            if *dir { "icons/folder.svg" } else { "icons/file.svg" },
            name.clone(),
            Path::new(path).parent().map(|parent| parent.to_string_lossy().into_owned()).unwrap_or_default(),
            t!("Abrir em nova aba"),
        ),
    };
    let draggable = row.path().is_some();
    div()
        .id(("palette-row", index))
        .h(px(ROW))
        .px(px(10.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .rounded(px(6.0))
        .cursor_pointer()
        .when(selected, |this| this.bg(rgb(theme::bg_hover())))
        .when(!selected, |this| this.hover(|this| this.bg(rgb(theme::bg_hover()))))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |page, event: &MouseDownEvent, _, cx| {
                if let Some(palette) = page.palette.as_mut() {
                    palette.selected = index;
                    palette.press = draggable.then_some((index, event.position));
                    cx.notify();
                }
            }),
        )
        .on_click(cx.listener(move |page, _, window, cx| page.activate_palette_row(Some(index), window, cx)))
        .child(Icon::new(Icon::empty()).path(icon).size_4().flex_shrink_0().text_color(rgb(if matches!(row, Row::Tab { .. }) {
            theme::accent()
        } else {
            theme::fg_secondary()
        })))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .items_baseline()
                .gap(px(8.0))
                .overflow_hidden()
                .child(div().flex_shrink_0().max_w(px(320.0)).text_sm().text_color(rgb(theme::fg())).overflow_hidden().text_ellipsis().whitespace_nowrap().child(name))
                .when(!detail.is_empty(), |this| {
                    this.child(
                        div()
                            .min_w(px(0.0))
                            .text_xs()
                            .text_color(rgb(theme::muted()))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(detail),
                    )
                }),
        )
        .when(selected || matches!(row, Row::Tab { .. }), |this| {
            this.child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .text_xs()
                    .text_color(rgb(theme::muted()))
                    .child(hint)
                    .child(
                        div()
                            .size(px(20.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.0))
                            .bg(rgb(theme::bg_hover()))
                            .child(Icon::new(IconName::ArrowRight).size_3()),
                    ),
            )
        })
        .into_any_element()
}
