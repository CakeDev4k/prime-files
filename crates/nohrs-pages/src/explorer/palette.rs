//! Ctrl+T: a search box over the window, as Zen Browser's new tab is. It
//! lists the open tabs that match (to go to one instead of opening it twice)
//! and every file and folder of the computer whose name matches, from the
//! whole-computer index. Enter or a click opens the hit in a new tab — a
//! folder shows itself, a file its folder with it selected — and a hit can be
//! dragged out like a file of the listing. With nothing typed, Enter opens a
//! plain new tab, which is what Ctrl+T and the "+" did before.
//!
//! The hits are drawn as the explorer draws files: Windows' icons, the
//! pictures of images and videos, and the waveforms of sounds. Ctrl+wheel,
//! Ctrl+= / Ctrl+- / Ctrl+0 or the − / + of the box change their size, from
//! a list of small icons to a grid of large pictures; the size is kept for
//! the next time.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::prelude::*;
use gpui::*;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{Icon, IconName};
use nohrs_core::{t, tf};
use nohrs_core::telemetry::LogErr;
use nohrs_ui::theme::theme;

use super::page::ExplorerPage;
use super::thumbnails::Thumbnails;
use super::waveforms::Waveforms;

// How many hits of the index the box shows.
const HITS: usize = 60;
// How far the pointer moves with the button down before a press is a drag.
const DRAG_THRESHOLD: f32 = 6.0;

// The size of the box's pictures, as one number from 0 (a list of small
// icons) to 1 (a grid of large pictures). See `icon_size`.
const ZOOM_DEFAULT: f32 = 0.1;
const ZOOM_STEP: f32 = 0.08;
// The icon size, in pixels, from which the hits are a grid of tiles.
const GRID_FROM: f32 = 64.0;
// The smallest icon size the list shows pictures at, rather than icons:
// below it a photo is a smudge and the type's icon reads better.
const PICTURES_FROM: f32 = 32.0;
// A row of the list holds its icon and this much around it.
const ROW_PADDING: f32 = 14.0;
const ROW_MIN: f32 = 40.0;
// A tile holds its picture, two lines of name, and this around them.
const TILE_PADDING: f32 = 10.0;
const TILE_NAME: f32 = 42.0;
// The box as it was before it had sizes: its width, and a row's height.
const PLAIN_WIDTH: f32 = 680.0;
const PLAIN_ROW: f32 = 46.0;

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
    // Whether the arrows chose a row since the text last changed. With
    // nothing typed and no row chosen, Enter makes a plain new tab rather
    // than going to the first tab listed.
    chose: bool,
    // Tiles per line in the last frame drawn as a grid; 0 as a list. The
    // arrows move by a line of tiles.
    columns: usize,
    // For each row, the child of the scrolled list that holds it, as last
    // drawn: what `scroll_to_item` is given.
    line_of: Vec<usize>,
}

#[derive(Clone)]
enum Row {
    /// An open tab: its entity id, title and folder.
    Tab { id: u64, title: String, folder: String },
    /// A path typed in full that exists.
    Typed { path: String, dir: bool },
    /// A hit of the index, with its size and modification time (seconds),
    /// which a waveform is kept by.
    Hit { name: String, path: String, dir: bool, size: u64, modified: u64 },
}

impl Row {
    fn path(&self) -> Option<(&str, bool)> {
        match self {
            Row::Tab { .. } => None,
            Row::Typed { path, dir } | Row::Hit { path, dir, .. } => Some((path, *dir)),
        }
    }

    fn is_hit(&self) -> bool {
        matches!(self, Row::Hit { .. })
    }
}

/// The pictures of the box, kept by the page between openings so a search
/// made again does not ask Windows for them again.
#[derive(Default)]
pub(crate) struct PaletteMedia {
    thumbnails: Thumbnails,
    waveforms: Waveforms,
    zoom: Option<f32>,
}

// Icon size in pixels for a zoom: 20 px at 0, 220 at 1.
fn icon_size(zoom: f32) -> f32 {
    20.0 + zoom.clamp(0.0, 1.0) * 200.0
}

// The zoom shown as a percentage of the default size.
fn zoom_percent(zoom: f32) -> u32 {
    (icon_size(zoom) / icon_size(ZOOM_DEFAULT) * 100.0).round() as u32
}

fn zoom_file() -> PathBuf {
    nohrs_core::config::paths::data_dir().join("palette-zoom")
}

#[allow(clippy::disallowed_methods)]
fn load_zoom() -> f32 {
    std::fs::read_to_string(zoom_file())
        .ok()
        .and_then(|text| text.trim().parse::<f32>().ok())
        .filter(|zoom| (0.0..=1.0).contains(zoom))
        .unwrap_or(ZOOM_DEFAULT)
}

fn save_zoom(zoom: f32, cx: &mut App) {
    cx.background_spawn(async move {
        let path = zoom_file();
        let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| {
            #[allow(clippy::disallowed_methods)]
            std::fs::write(&path, format!("{zoom:.3}"))
        });
        if let Err(error) = written {
            tracing::warn!("could not keep the search box's size: {error}");
        }
    })
    .detach();
}

impl ExplorerPage {
    fn palette_zoom(&mut self) -> f32 {
        *self.palette_media.zoom.get_or_insert_with(load_zoom)
    }

    // Changes the pictures' size by `step` (or back to the default with
    // `None`), keeping the selected row in view.
    fn zoom_palette(&mut self, step: Option<f32>, cx: &mut Context<Self>) {
        let now = self.palette_zoom();
        let next = step.map_or(ZOOM_DEFAULT, |step| (now + step).clamp(0.0, 1.0));
        if (next - now).abs() < f32::EPSILON {
            return;
        }
        self.palette_media.zoom = Some(next);
        save_zoom(next, cx);
        if let Some(palette) = self.palette.as_mut() {
            // Lines change with the size; the selected one is brought back
            // once the next frame has laid them out.
            palette.line_of.clear();
        }
        cx.notify();
    }

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
            chose: false,
            columns: 0,
            line_of: Vec::new(),
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
        rows.extend(palette.rows.iter().filter(|row| row.is_hit()).cloned());
        palette.rows = rows;
        palette.selected = 0;
        palette.chose = false;

        palette.cancel.store(true, Ordering::Relaxed);
        if needle.is_empty() {
            palette.rows.retain(|row| !row.is_hit());
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
            // Names gone since the index was made are left out; the size and
            // date of the others come with the same look at the disk.
            found
                .into_iter()
                .filter(|_| !cancel.load(Ordering::Relaxed))
                .filter_map(|(name, path, dir)| {
                    #[allow(clippy::disallowed_methods)]
                    let metadata = std::fs::metadata(&path).ok()?;
                    let modified = metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0, |since| since.as_secs());
                    Some(Row::Hit { name, path, dir, size: metadata.len(), modified })
                })
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
                palette.rows.retain(|row| !row.is_hit());
                palette.rows.extend(found);
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

    // Up/down (`vertical`) move by a line: one row of the list, or one line
    // of tiles among the hits of the grid. Left/right move by one, once the
    // arrows are choosing (before that they move the text cursor).
    fn move_palette_selection(&mut self, step: isize, vertical: bool, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.as_mut() else {
            return;
        };
        if palette.rows.is_empty() {
            return;
        }
        let last = palette.rows.len() as isize - 1;
        let first_hit = palette.rows.iter().position(Row::is_hit);
        let current = palette.selected as isize;
        let target = if !palette.chose && palette.query.trim().is_empty() {
            // The first press lands on the first row, which showed unlit.
            0
        } else if !palette.chose {
            current
        } else if vertical && palette.columns > 1 && let Some(first_hit) = first_hit {
            let first_hit = first_hit as isize;
            let columns = palette.columns as isize;
            if current >= first_hit {
                let moved = current + step * columns;
                if moved < first_hit {
                    // Up from the first line of tiles: the row above them.
                    if first_hit > 0 { first_hit - 1 } else { current }
                } else if moved > last {
                    // Down from a short last line keeps to the last tile.
                    if (current - first_hit) / columns < (last - first_hit) / columns { last } else { current }
                } else {
                    moved
                }
            } else {
                current + step.signum()
            }
        } else {
            current + step
        };
        palette.selected = target.clamp(0, last) as usize;
        palette.chose = true;
        if let Some(&line) = palette.line_of.get(palette.selected) {
            palette.scroll.scroll_to_item(line);
        }
        cx.notify();
    }

    // Enter (`None`: the selected row) or a click on `row`.
    fn activate_palette_row(&mut self, row: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.as_ref() else {
            return;
        };
        let empty = palette.query.trim().is_empty();
        let chosen = match row {
            Some(row) => palette.rows.get(row).cloned(),
            None if empty && !palette.chose => None,
            None => palette.rows.get(palette.selected).cloned(),
        };
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

    // What the hit at `path` shows in a square of `icon` pixels: its
    // waveform (a sound, with waveforms on), its picture (big enough for one
    // to read) or Windows' icon. The caches are the page's own: the page
    // draws the box, and what arrives redraws the page.
    fn palette_media(&mut self, row: &Row, icon: f32, scale: f32, cx: &mut Context<Self>) -> Media {
        // A tab shows the icon of the folder it is on, as the folder would
        // show in the listing.
        let (path, dir) = match row {
            Row::Tab { folder, .. } => (folder.as_str(), true),
            _ => match row.path() {
                Some(found) => found,
                None => return Media::None,
            },
        };
        let pixels = ((icon * scale).round() as u32).max(16);
        if let Row::Hit { size, modified, .. } = row
            && !dir
            && icon >= PICTURES_FROM
            && self.ui.audio_waveforms
            && super::waveforms::is_audio(path)
            && let Some(bars) =
                self.palette_media.waveforms.get_for(path, *size, *modified, |page: &mut Self| &mut page.palette_media.waveforms, cx)
        {
            return Media::Waveform(bars);
        }
        let image = if dir || icon < PICTURES_FROM {
            self.palette_media.thumbnails.icon_for(path, dir, pixels, |page: &mut Self| &mut page.palette_media.thumbnails, cx)
        } else {
            let bucket = super::zoom::thumbnail_bucket(icon, scale);
            self.palette_media.thumbnails.get_for(path, bucket, |page: &mut Self| &mut page.palette_media.thumbnails, cx)
        };
        image.map_or(Media::None, Media::Image)
    }

    /// The box over the window, when it is open.
    pub(crate) fn render_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.palette.as_ref()?;
        let zoom = self.palette_zoom();
        let icon = icon_size(zoom);
        let grid = icon >= GRID_FROM;
        let scale = window.scale_factor();
        // What the last frame did not ask for scrolled away or is gone.
        self.palette_media.thumbnails.begin_frame(cx);
        self.palette_media.waveforms.begin_frame();

        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        // Without files found (nothing typed: the open tabs) the box is the
        // one Ctrl+T always was, whatever the size chosen for files: the
        // size is about pictures, and tabs have none.
        let plain = self.palette.as_ref()?.rows.iter().all(|row| !row.is_hit());
        // One width for the list and the grid: changing the size must not
        // move the − / + out from under the pointer.
        let width = (vw - 48.0).clamp(280.0, if plain { PLAIN_WIDTH } else { 960.0 });
        let left = (vw - width) / 2.0;
        let top = (vh * 0.10).clamp(16.0, 96.0);
        // Room for the list under the field and above the footer.
        let list_max = (vh - top - 150.0).max(120.0);
        // The scrolled area's inner width: the card's, less its padding and
        // the scrollbar's lane.
        let inner = width - 2.0 * 6.0 - 10.0;
        let columns = if grid { ((inner / (icon + 2.0 * TILE_PADDING)).floor() as usize).max(1) } else { 0 };
        // The tiles share the line's width, so the grid has no gap at its end.
        let tile_width = if grid { (inner / columns as f32).floor() } else { 0.0 };
        let row_height = (icon + ROW_PADDING).max(ROW_MIN);

        let (rows, selected, lit_any, query_empty) = {
            let palette = self.palette.as_ref()?;
            (
                palette.rows.clone(),
                palette.selected,
                palette.chose || !palette.query.trim().is_empty(),
                palette.query.trim().is_empty(),
            )
        };
        let tab_count = rows.iter().filter(|row| matches!(row, Row::Tab { .. })).count();
        let hit_count = rows.iter().filter(|row| row.is_hit()).count();

        // The scrolled list, as lines: headings, rows, and lines of tiles.
        let mut lines: Vec<AnyElement> = Vec::new();
        let mut line_of = vec![0; rows.len()];
        // Headings only beside files: the plain box is the tabs alone.
        if tab_count > 0 && !plain {
            lines.push(heading(t!("Abas abertas"), tab_count));
        }
        let mut index = 0;
        // Tabs and a typed path are always rows.
        while index < rows.len() && !rows[index].is_hit() {
            if index == tab_count && matches!(rows[index], Row::Typed { .. }) && tab_count > 0 {
                lines.push(div().h(px(6.0)).into_any_element());
            }
            let lit = lit_any && index == selected;
            line_of[index] = lines.len();
            let media = self.palette_media(&rows[index], 16.0, scale, cx);
            lines.push(render_row(&rows[index], index, lit, media, PLAIN_ROW, 16.0, inner, cx));
            index += 1;
        }
        if hit_count > 0 {
            lines.push(heading(t!("No computador"), hit_count));
        }
        let hits: Vec<usize> = (index..rows.len()).collect();
        if grid {
            for chunk in hits.chunks(columns) {
                let line = lines.len();
                let mut tiles = Vec::with_capacity(chunk.len());
                for &at in chunk {
                    line_of[at] = line;
                    let media = self.palette_media(&rows[at], icon, scale, cx);
                    tiles.push(render_tile(&rows[at], at, lit_any && at == selected, media, icon, tile_width, cx));
                }
                lines.push(div().flex().items_start().children(tiles).into_any_element());
            }
        } else {
            for at in hits {
                line_of[at] = lines.len();
                let media = self.palette_media(&rows[at], icon, scale, cx);
                lines.push(render_row(&rows[at], at, lit_any && at == selected, media, row_height, icon, inner, cx));
            }
        }
        let empty = rows.is_empty();

        let palette = self.palette.as_mut()?;
        let relaid = palette.line_of.is_empty() && !line_of.is_empty();
        palette.line_of = line_of;
        palette.columns = columns;
        if relaid && palette.chose && let Some(&line) = palette.line_of.get(palette.selected) {
            palette.scroll.scroll_to_item(line);
        }
        let palette = self.palette.as_ref()?;
        let searching = !query_empty && hit_count == 0;
        // A first indexing still under way says how far it got instead.
        let indexing = cx
            .try_global::<super::everything::Everything>()
            .and_then(|state| state.building.as_ref())
            .map(|found| found.load(Ordering::Relaxed));

        let card = div()
            .id("palette")
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .flex()
            .flex_col()
            .rounded(px(14.0))
            .border_1()
            .border_color(rgb(theme::border()))
            .bg(rgb(theme::bg_secondary()))
            .shadow_2xl()
            .overflow_hidden()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(|page, event: &KeyDownEvent, window, cx| {
                let keys = &event.keystroke;
                let ctrl = keys.modifiers.control || keys.modifiers.platform;
                let choosing = page.palette.as_ref().is_some_and(|palette| palette.chose && palette.columns > 1);
                match keys.key.as_str() {
                    "=" | "+" if ctrl => page.zoom_palette(Some(ZOOM_STEP), cx),
                    "-" if ctrl => page.zoom_palette(Some(-ZOOM_STEP), cx),
                    "0" if ctrl => page.zoom_palette(None, cx),
                    "down" => page.move_palette_selection(1, true, cx),
                    "up" => page.move_palette_selection(-1, true, cx),
                    "right" if choosing => page.move_palette_selection(1, false, cx),
                    "left" if choosing => page.move_palette_selection(-1, false, cx),
                    "pagedown" => page.move_palette_selection(6, true, cx),
                    "pageup" => page.move_palette_selection(-6, true, cx),
                    "escape" => page.close_palette(window, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            // Ctrl+wheel changes the size, as in the listing; the wheel alone
            // scrolls.
            .on_scroll_wheel(cx.listener(|page, event: &ScrollWheelEvent, window, cx| {
                if !(event.modifiers.control || event.modifiers.platform) {
                    return;
                }
                let delta = f32::from(event.delta.pixel_delta(window.line_height()).y);
                if delta.abs() < 0.5 {
                    return;
                }
                page.zoom_palette(Some(if delta > 0.0 { ZOOM_STEP / 2.0 } else { -ZOOM_STEP / 2.0 }), cx);
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
                    .h(px(56.0))
                    .px(px(16.0))
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(Icon::new(IconName::Search).size_5().text_color(rgb(if query_empty {
                        theme::muted()
                    } else {
                        theme::accent()
                    })))
                    .child(div().flex_1().min_w(px(0.0)).text_base().child(Input::new(&palette.input).appearance(false)))
                    .when(!plain, |this| this.child(zoom_control(zoom, cx))),
            )
            .when(!empty, |card| {
                card.child(div().h(px(1.0)).bg(rgb(theme::border()))).child(
                    div()
                        .id("palette-rows")
                        .max_h(px(list_max))
                        .px(px(6.0))
                        .py(px(6.0))
                        .overflow_y_scroll()
                        .track_scroll(&palette.scroll)
                        .children(lines),
                )
            })
            .when(searching, |card| {
                card.child(
                    div()
                        .px(px(18.0))
                        .py(px(10.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .text_xs()
                        .text_color(rgb(theme::muted()))
                        .child(Icon::new(Icon::empty()).path("icons/loader.svg").size_3())
                        .child(match indexing {
                            Some(found) => tf!("Indexando o computador… {n} itens", n = found),
                            None => t!("Buscando no computador…").to_string(),
                        }),
                )
            })
            .child(
                div()
                    .px(px(14.0))
                    .py(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(14.0))
                    .border_t_1()
                    .border_color(rgb(theme::border()))
                    .bg(rgb(theme::bg()))
                    .text_xs()
                    .text_color(rgb(theme::muted()))
                    .child(key_hint("↑↓", t!("Escolher")))
                    .child(key_hint("Enter", t!("Abrir em nova aba")))
                    .child(key_hint("Ctrl+roda", t!("Tamanho")))
                    .child(key_hint("Esc", t!("Fechar")))
                    .child(div().flex_1())
                    .child(div().text_color(rgb(theme::muted())).child(t!("Arraste para levar o arquivo"))),
            );

        Some(
            deferred(
                anchored().position_mode(AnchoredPositionMode::Window).position(point(px(0.0), px(0.0))).child(
                    div()
                        .id("palette-backdrop")
                        .w(px(vw))
                        .h(px(vh))
                        .bg(rgba(0x0000_0059))
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

// What a hit shows in its square.
enum Media {
    None,
    Image(Arc<RenderImage>),
    Waveform(Arc<[u8]>),
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

// A group's name over its rows, with how many there are.
fn heading(label: &str, count: usize) -> AnyElement {
    div()
        .px(px(10.0))
        .pt(px(8.0))
        .pb(px(4.0))
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::muted()))
        .child(label.to_string())
        .child(div().font_weight(FontWeight::NORMAL).child(count.to_string()))
        .into_any_element()
}

// A key and what it does, for the footer.
fn key_hint(key: &'static str, label: &str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(5.0))
        .child(
            div()
                .px(px(5.0))
                .py(px(1.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(rgb(theme::border()))
                .bg(rgb(theme::bg_secondary()))
                .text_color(rgb(theme::fg_secondary()))
                .child(key),
        )
        .child(label.to_string())
}

// The − 100% + at the end of the field.
fn zoom_control(zoom: f32, cx: &mut Context<ExplorerPage>) -> impl IntoElement {
    let button = |id: &'static str, icon: IconName, enabled: bool| {
        div()
            .id(id)
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.0))
            .when(enabled, |this| this.cursor_pointer().hover(|style| style.bg(rgb(theme::bg_hover()))))
            .when(!enabled, |this| this.opacity(0.35))
            .child(Icon::new(icon).size_3().text_color(rgb(theme::fg_secondary())))
    };
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(2.0))
        .px(px(4.0))
        .py(px(2.0))
        .rounded(px(8.0))
        .bg(rgb(theme::bg()))
        .child(
            button("palette-zoom-out", IconName::Minus, zoom > 0.0)
                .on_click(cx.listener(|page, _, _, cx| page.zoom_palette(Some(-ZOOM_STEP), cx))),
        )
        .child(
            div()
                .id("palette-zoom-reset")
                .w(px(40.0))
                .text_xs()
                .text_center()
                .cursor_pointer()
                .text_color(rgb(theme::fg_secondary()))
                .on_click(cx.listener(|page, _, _, cx| page.zoom_palette(None, cx)))
                .child(format!("{}%", zoom_percent(zoom))),
        )
        .child(
            button("palette-zoom-in", IconName::Plus, zoom < 1.0)
                .on_click(cx.listener(|page, _, _, cx| page.zoom_palette(Some(ZOOM_STEP), cx))),
        )
}

// The square a row or tile draws its picture in.
fn media_square(row: &Row, media: Media, icon: f32, selected: bool) -> AnyElement {
    let square = div().size(px(icon)).flex_shrink_0().flex().items_center().justify_center();
    match media {
        Media::Image(image) => square
            .child(img(ImageSource::Render(image)).size_full().object_fit(ObjectFit::Contain))
            .into_any_element(),
        Media::Waveform(bars) => square
            .rounded(px(6.0))
            .bg(rgb(theme::bg()))
            .child(div().size_full().child(super::waveforms::canvas(bars, icon, selected, None)))
            .into_any_element(),
        Media::None => {
            let (path, color) = match row {
                Row::Tab { .. } | Row::Typed { dir: true, .. } | Row::Hit { dir: true, .. } => {
                    ("icons/folder.svg", theme::accent())
                }
                _ => ("icons/file.svg", theme::fg_secondary()),
            };
            square
                .child(Icon::new(Icon::empty()).path(path).size(px((icon * 0.8).clamp(16.0, 96.0))).text_color(rgb(color)))
                .into_any_element()
        }
    }
}

// Selects the row on a press, readying a drag; a click opens it.
fn pressable<E: InteractiveElement + StatefulInteractiveElement>(
    element: E,
    index: usize,
    draggable: bool,
    cx: &mut Context<ExplorerPage>,
) -> E {
    element
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
}

fn render_row(
    row: &Row,
    index: usize,
    selected: bool,
    media: Media,
    height: f32,
    icon: f32,
    width: f32,
    cx: &mut Context<ExplorerPage>,
) -> AnyElement {
    let (name, detail, hint): (String, String, &'static str) = match row {
        Row::Tab { title, folder, .. } => (title.clone(), folder.clone(), t!("Ir para a aba")),
        Row::Typed { path, .. } => (path.clone(), String::new(), t!("Abrir em nova aba")),
        Row::Hit { name, path, .. } => (
            name.clone(),
            Path::new(path).parent().map(|parent| parent.to_string_lossy().into_owned()).unwrap_or_default(),
            t!("Abrir em nova aba"),
        ),
    };
    // Two lines (name over folder) once the icon is tall enough for them.
    let stacked = icon >= 32.0;
    // The folder is cut in its middle to the room it has, keeping the drive
    // and the folder it is in: the text's own ellipsis measured that room
    // as nothing in the stacked layout and showed only "…". About 6.5 px a
    // character at the size it is drawn; the hint takes ~150 px when shown.
    let hinted = selected || matches!(row, Row::Tab { .. });
    let room = width - icon - 60.0 - if stacked { 0.0 } else { 180.0 } - if hinted { 150.0 } else { 0.0 };
    let detail = super::view::listing::truncate_middle(&detail, ((room / 6.5) as usize).max(12));
    let draggable = row.path().is_some();
    let element = div()
        .id(("palette-row", index))
        .relative()
        .h(px(height))
        .px(px(10.0))
        .flex()
        .items_center()
        .gap(px(12.0))
        .rounded(px(8.0))
        .cursor_pointer()
        .when(selected, |this| this.bg(rgb(theme::bg_hover())))
        .when(!selected, |this| this.hover(|this| this.bg(rgb(theme::bg_hover()))))
        // A bar of the accent at the selected row's edge.
        .when(selected, |this| {
            this.child(
                div()
                    .absolute()
                    .left(px(2.0))
                    .top(px(height * 0.25))
                    .h(px(height * 0.5))
                    .w(px(3.0))
                    .rounded(px(2.0))
                    .bg(rgb(theme::accent())),
            )
        })
        .child(media_square(row, media, icon, selected))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .when(stacked, |this| this.flex_col().gap(px(2.0)))
                .when(!stacked, |this| this.items_baseline().gap(px(8.0)))
                .overflow_hidden()
                .child(
                    div()
                        .when(stacked, |this| this.w_full())
                        .when(!stacked, |this| this.flex_shrink_0().max_w(px(320.0)))
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(theme::fg()))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(name),
                )
                .when(!detail.is_empty(), |this| {
                    this.child(
                        div()
                            .min_w(px(0.0))
                            .when(stacked, |this| this.w_full())
                            .text_xs()
                            .text_color(rgb(theme::muted()))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(detail),
                    )
                }),
        )
        // Every tab says it goes to the tab, as it always did; a file only
        // when chosen.
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
                            .bg(rgb(theme::bg()))
                            .child(Icon::new(IconName::ArrowRight).size_3()),
                    ),
            )
        });
    pressable(element, index, draggable, cx).into_any_element()
}

// A hit of the grid: its picture over its name, the folder in the tooltip.
fn render_tile(
    row: &Row,
    index: usize,
    selected: bool,
    media: Media,
    icon: f32,
    width: f32,
    cx: &mut Context<ExplorerPage>,
) -> AnyElement {
    let (name, path) = match row {
        Row::Hit { name, path, .. } => (name.clone(), path.clone()),
        _ => (String::new(), String::new()),
    };
    let tip: SharedString = path.into();
    let element = div()
        .id(("palette-tile", index))
        .w(px(width))
        .h(px(icon + 2.0 * TILE_PADDING + TILE_NAME))
        .p(px(TILE_PADDING))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.0))
        .rounded(px(10.0))
        .border_1()
        .cursor_pointer()
        .border_color(if selected { rgb(theme::accent()) } else { rgba(0x0000_0000) })
        .when(selected, |this| this.bg(rgb(theme::bg_hover())))
        .when(!selected, |this| this.hover(|this| this.bg(rgb(theme::bg_hover()))))
        .tooltip(move |window, cx| gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx))
        .child(media_square(row, media, icon, selected))
        .child(
            div()
                .w_full()
                .h(px(TILE_NAME - 6.0))
                .text_xs()
                .line_height(px(17.0))
                .text_center()
                .line_clamp(2)
                .overflow_hidden()
                .text_color(rgb(theme::fg()))
                .child(name),
        );
    pressable(element, index, true, cx).into_any_element()
}

#[cfg(test)]
mod tests {
    // Not `super::*`: it brings gpui's `test` attribute over the std one.
    use super::{GRID_FROM, PICTURES_FROM, ZOOM_DEFAULT, folder_and_selection, icon_size, zoom_percent};

    #[test]
    fn the_box_turns_into_a_grid_past_the_list_sizes() {
        assert!(icon_size(0.0) < PICTURES_FROM, "the smallest size is plain icons");
        assert!(icon_size(ZOOM_DEFAULT) >= PICTURES_FROM, "the default shows pictures");
        assert!(icon_size(ZOOM_DEFAULT) < GRID_FROM, "the default is a list");
        assert!(icon_size(1.0) > GRID_FROM, "the largest size is a grid");
        assert_eq!(zoom_percent(ZOOM_DEFAULT), 100);
    }

    #[test]
    fn a_file_opens_its_folder_with_it_selected() {
        let (folder, select) = folder_and_selection(r"C:\a\b.txt", false);
        assert_eq!(folder, r"C:\a");
        assert_eq!(select.as_deref(), Some(r"C:\a\b.txt"));
        assert_eq!(folder_and_selection(r"C:\a", true), (r"C:\a".to_string(), None));
    }
}
