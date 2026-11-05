//! The listing with the sidebar and the preview docked around it.
//!
//! Each of the two docks to any side of the listing (`[layout]` in the
//! config, or carried there by its grip). A panel on the left or the right is
//! a column and keeps the width it was dragged to; on the top or the bottom
//! it is a band and keeps its height; the listing takes what is left, as in
//! Explorer: widening the window widens the listing. This replaced
//! gpui-component's resizable group, which on every change of the window or
//! of a panel shared the space out again in proportion and ignored the
//! panels' limits, so the sidebar grew past its maximum and the listing
//! could be squeezed to nothing.
//!
//! The gestures are `crate::dock`'s: the inner edge resizes and, pulled past
//! half the smallest size, hides the panel; a hidden panel's side keeps a
//! strip that drags it open; the grip carries it to another side; an edge
//! no hidden panel claims drags out a second pane.

use std::cell::Cell;

use gpui::prelude::FluentBuilder;
use gpui::*;
use nohrs_core::config::DockSide;

use crate::dock::{self, Gesture, Resized};
use crate::explorer::ExplorerPane;
use crate::explorer::types::PaneEvent;

/// Room always left to the listing between the panels: across, and down.
const LISTING_MIN: f32 = 220.0;
const LISTING_MIN_HEIGHT: f32 = 120.0;

/// A panel docked around the listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Panel {
    Sidebar,
    Preview,
}

impl Panel {
    const ALL: [Panel; 2] = [Panel::Sidebar, Panel::Preview];

    fn index(self) -> usize {
        match self {
            Panel::Sidebar => 0,
            Panel::Preview => 1,
        }
    }

    /// The smallest and largest size as a column (`true`) or a band.
    fn range(self, column: bool) -> (f32, f32) {
        match (self, column) {
            (Panel::Sidebar, true) => (160.0, 520.0),
            (Panel::Sidebar, false) => (96.0, 600.0),
            (Panel::Preview, true) => (240.0, 4000.0),
            (Panel::Preview, false) => (140.0, 4000.0),
        }
    }

    fn default_size(self, column: bool) -> f32 {
        match (self, column) {
            (Panel::Sidebar, true) => 240.0,
            (Panel::Sidebar, false) => 170.0,
            (Panel::Preview, true) => 340.0,
            (Panel::Preview, false) => 280.0,
        }
    }

    /// The `[layout]` key that says where it docks.
    pub(crate) fn config_key(self) -> &'static str {
        match self {
            Panel::Sidebar => "sidebar",
            Panel::Preview => "preview",
        }
    }
}

/// Where the two panels dock and the sizes they were dragged to, as a column
/// and as a band, so carrying one to the other kind of side and back finds
/// its size again.
#[derive(Clone, Copy)]
struct Arrangement {
    sides: [DockSide; 2],
    // [panel][0 = column width, 1 = band height]
    sizes: [[f32; 2]; 2],
}

thread_local! {
    // Shared by every tab, as the panes of Explorer's windows are: a new tab
    // opens with the sides and sizes the last drag left.
    static ARRANGEMENT: Cell<Arrangement> = const {
        Cell::new(Arrangement {
            sides: [DockSide::Left, DockSide::Right],
            sizes: [[240.0, 170.0], [340.0, 280.0]],
        })
    };
}

fn slot(side: DockSide) -> usize {
    if side.is_column() { 0 } else { 1 }
}

/// The side `panel` docks to.
pub(crate) fn side(panel: Panel) -> DockSide {
    ARRANGEMENT.get().sides[panel.index()]
}

/// Docks the panels where `[layout]` says.
pub(crate) fn set_sides(sidebar: DockSide, preview: DockSide) {
    let mut arrangement = ARRANGEMENT.get();
    arrangement.sides = [sidebar, preview];
    ARRANGEMENT.set(arrangement);
}

/// The size `panel` was left at on its side, before fitting.
fn size(panel: Panel) -> f32 {
    let arrangement = ARRANGEMENT.get();
    arrangement.sizes[panel.index()][slot(arrangement.sides[panel.index()])]
}

fn set_size(panel: Panel, value: f32) {
    let mut arrangement = ARRANGEMENT.get();
    let slot = slot(arrangement.sides[panel.index()]);
    arrangement.sizes[panel.index()][slot] = value;
    ARRANGEMENT.set(arrangement);
}

fn range(panel: Panel) -> (f32, f32) {
    panel.range(side(panel).is_column())
}

/// The preview's width as a column, in logical pixels: what a fitted image
/// is asked of the Shell for. As a band the preview is about as wide as the
/// listing, which the window's width bounds the same way.
pub(crate) fn preview_width() -> f32 {
    let arrangement = ARRANGEMENT.get();
    if arrangement.sides[Panel::Preview.index()].is_column() {
        arrangement.sizes[Panel::Preview.index()][0]
    } else {
        arrangement.sizes[Panel::Preview.index()][1] * 2.0
    }
}

/// The width the sidebar has as a column, for compact mode, where it floats
/// over the listing at that width.
pub(crate) fn sidebar_width() -> f32 {
    let (min, max) = Panel::Sidebar.range(true);
    ARRANGEMENT.get().sizes[Panel::Sidebar.index()][0].clamp(min, max)
}

/// A drag of a panel's edge or grip, or of an edge that splits the listing
/// (`panel` is `None` then).
#[derive(Clone, Copy, Debug)]
pub(crate) struct DockDrag {
    pub panel: Option<Panel>,
    pub side: DockSide,
    pub gesture: Gesture,
    pub pointer: Point<Pixels>,
    /// Pulled under half its smallest size: hidden when the button comes up.
    pub collapsing: bool,
}

impl ExplorerPane {
    fn panel_visible(&self, panel: Panel) -> bool {
        match panel {
            Panel::Sidebar => self.sidebar_visible,
            Panel::Preview => self.preview_visible,
        }
    }

    fn set_panel_visible(&mut self, panel: Panel, visible: bool) {
        match panel {
            Panel::Sidebar => self.sidebar_visible = visible,
            Panel::Preview => self.preview_visible = visible,
        }
    }

    /// Starts a drag of `panel`'s inner edge from `pointer`.
    pub(crate) fn start_panel_resize(&mut self, panel: Panel, pointer: Point<Pixels>) {
        let side = side(panel);
        self.dock_drag = Some(DockDrag {
            panel: Some(panel),
            side,
            gesture: Gesture::Resize {
                start: dock::along(side, pointer),
                start_size: size(panel),
            },
            pointer,
            collapsing: false,
        });
    }

    /// Starts pulling the `side` edge: a hidden `panel` out of it, or a
    /// second pane when `panel` is `None`.
    pub(crate) fn start_edge_pull(&mut self, panel: Option<Panel>, side: DockSide, pointer: Point<Pixels>) {
        self.dock_drag = Some(DockDrag {
            panel,
            side,
            gesture: Gesture::Open,
            pointer,
            collapsing: false,
        });
    }

    /// Starts carrying `panel` by its grip.
    pub(crate) fn start_panel_move(&mut self, panel: Panel, pointer: Point<Pixels>) {
        self.dock_drag = Some(DockDrag {
            panel: Some(panel),
            side: side(panel),
            gesture: Gesture::Move,
            pointer,
            collapsing: false,
        });
    }

    /// Follows a dock drag to `pointer`. Returns whether one is under way.
    pub(crate) fn update_dock_drag(&mut self, pointer: Point<Pixels>) -> bool {
        let Some(mut drag) = self.dock_drag else {
            return false;
        };
        drag.pointer = pointer;
        match (drag.gesture, drag.panel) {
            (Gesture::Resize { start, start_size }, Some(panel)) => {
                let (min, max) = range(panel);
                let raw = start_size + (dock::along(drag.side, pointer) - start) * dock::growth(drag.side);
                match dock::resized(raw, min, max) {
                    Resized::Size(value) => {
                        drag.collapsing = false;
                        set_size(panel, value);
                    }
                    Resized::Collapse => drag.collapsing = true,
                }
            }
            (Gesture::Open, Some(panel)) => {
                let pulled = dock::depth(drag.side, self.columns_bounds.get(), pointer);
                if pulled >= dock::OPEN_PULL {
                    let (min, max) = range(panel);
                    set_size(panel, pulled.clamp(min, max));
                    self.set_panel_visible(panel, true);
                    // From here on it is an ordinary resize of the open panel,
                    // its edge under the pointer: from the depth pulled, not
                    // from the smallest size it was clamped to, which put the
                    // edge that far past the pointer.
                    drag.gesture = Gesture::Resize {
                        start: dock::along(drag.side, pointer),
                        start_size: pulled,
                    };
                }
            }
            _ => {}
        }
        self.dock_drag = Some(drag);
        true
    }

    /// Ends a dock drag: hides a panel pulled shut, docks a carried one where
    /// it was let go, or asks for a second pane. Returns whether one ended.
    pub(crate) fn finish_dock_drag(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(drag) = self.dock_drag.take() else {
            return false;
        };
        let bounds = self.columns_bounds.get();
        match (drag.gesture, drag.panel) {
            (Gesture::Resize { start_size, .. }, Some(panel)) if drag.collapsing => {
                // Hidden at the size it had, which it opens at again.
                set_size(panel, start_size);
                self.set_panel_visible(panel, false);
            }
            (Gesture::Move, Some(panel)) => {
                let target = dock::nearest_side(bounds, drag.pointer);
                if target != drag.side {
                    dock_panel(panel, target, cx);
                    cx.refresh_windows();
                }
            }
            (Gesture::Open, None) => {
                let depth = dock::depth(drag.side, bounds, drag.pointer);
                let across = if drag.side.is_column() { bounds.size.width } else { bounds.size.height };
                let across = f32::from(across);
                if depth >= LISTING_MIN.min(across / 3.0) && across > 0.0 {
                    cx.emit(PaneEvent::SplitRequested {
                        side: drag.side,
                        fraction: (depth / across).clamp(0.15, 0.85),
                    });
                }
            }
            _ => {}
        }
        true
    }

    /// Back to the default size, from a double click on the panel's edge.
    pub(crate) fn reset_panel_size(&mut self, panel: Panel) {
        set_size(panel, panel.default_size(side(panel).is_column()));
    }
}

/// Docks `panel` on `side` now, in every tab, and remembers it in the config
/// so the next start opens that way. The write goes off the UI thread; the
/// config watcher then applies the same sides again, which changes nothing.
pub(crate) fn dock_panel(panel: Panel, side: DockSide, cx: &mut App) {
    let mut arrangement = ARRANGEMENT.get();
    arrangement.sides[panel.index()] = side;
    ARRANGEMENT.set(arrangement);
    save_layout(panel.config_key(), format!("\"{}\"", side.as_str()), cx);
}

/// Writes `layout.<key> = value` to the config file in the background.
pub(crate) fn save_layout(key: &'static str, value: String, cx: &mut App) {
    cx.background_spawn(async move {
        if let Err(error) =
            nohrs_core::config::set_value(&nohrs_core::config::paths::config_file(), "layout", key, &value)
        {
            tracing::warn!("could not save layout.{key}: {error}");
        }
    })
    .detach();
}

/// A panel to lay out around the listing, and how far out compact mode has
/// slid it (1.0 otherwise); one part way out has no edge to drag.
pub(crate) struct Docked {
    pub panel: Panel,
    pub element: AnyElement,
    pub amount: f32,
}

/// The listing with its panels around it, each panel's edge with a strip to
/// drag it by, and the strips of hidden panels and of the split along the
/// area's edges.
pub(crate) fn render(
    page: &mut ExplorerPane,
    docked: Vec<Docked>,
    listing: AnyElement,
    cx: &mut Context<ExplorerPane>,
) -> AnyElement {
    let bounds = page.columns_bounds.get();
    let collapsing = page.dock_drag.filter(|drag| drag.collapsing).and_then(|drag| drag.panel);
    let resizing = page
        .dock_drag
        .filter(|drag| matches!(drag.gesture, Gesture::Resize { .. }))
        .and_then(|drag| drag.panel);

    // Fit the columns into the width and the bands into the height, the
    // sidebar (listed first) keeping its size longest.
    let mut fitted: Vec<(Panel, DockSide, f32, f32, AnyElement)> = Vec::new();
    for column in [true, false] {
        let mine: Vec<&Docked> = docked
            .iter()
            .filter(|item| side(item.panel).is_column() == column && collapsing != Some(item.panel))
            .collect();
        let mut sizes: Vec<(f32, f32)> = mine.iter().map(|item| (size(item.panel) * item.amount, 0.0)).collect();
        let ranges: Vec<(f32, f32)> = mine
            .iter()
            .map(|item| {
                let (min, max) = range(item.panel);
                (min * item.amount, max)
            })
            .collect();
        let (total, keep) = if column {
            (f32::from(bounds.size.width), LISTING_MIN)
        } else {
            (f32::from(bounds.size.height), LISTING_MIN_HEIGHT)
        };
        dock::fit(&mut sizes, &ranges, total, keep);
        for (item, (fitted_size, _)) in mine.iter().zip(sizes) {
            fitted.push((item.panel, side(item.panel), fitted_size, item.amount, div().into_any_element()));
        }
    }
    // The elements themselves, moved into their fitted places.
    for item in docked {
        if let Some(entry) = fitted.iter_mut().find(|entry| entry.0 == item.panel) {
            entry.4 = item.element;
        }
    }

    let dragging = page.dock_drag;
    // The columns beside the listing, which a band above or below it does
    // not reach: the landing of a panel carried there is drawn between them.
    let carried = dragging.filter(|drag| drag.gesture == Gesture::Move).and_then(|drag| drag.panel);
    let beside = |wanted: DockSide| -> f32 {
        fitted
            .iter()
            .filter(|entry| entry.1 == wanted && Some(entry.0) != carried)
            .map(|entry| entry.2)
            .sum()
    };
    let (left_room, right_room) = (beside(DockSide::Left), beside(DockSide::Right));
    let mut boxes: [Vec<AnyElement>; 4] = Default::default();
    for (panel, panel_side, fitted_size, amount, element) in fitted {
        // `amount` already shrank the size compact mode is sliding.
        let frame = dock::frame(panel_side, fitted_size, 1.0).when(amount <= 0.0, |frame| frame.border_0());
        let frame = frame.child(element);
        let frame = if amount >= 1.0 {
            frame.child(edge_strip(page, panel, panel_side, resizing == Some(panel), cx))
        } else {
            frame
        };
        boxes[side_index(panel_side)].push(frame.into_any_element());
    }
    let [left, right, top, bottom] = boxes;

    let center = div()
        .flex_1()
        .min_w(px(0.0))
        .h_full()
        .flex()
        .flex_col()
        .min_h(px(0.0))
        .overflow_hidden()
        .children(top)
        .child(
            div()
                .flex_1()
                .min_h(px(0.0))
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(listing),
        )
        // The panel listed first is the outermost on its side.
        .children(bottom.into_iter().rev());

    let mut edges: Vec<AnyElement> = Vec::new();
    for edge in [DockSide::Left, DockSide::Right, DockSide::Top, DockSide::Bottom] {
        // A hidden panel claims its side's edge; with none, the edge splits.
        let claimant = Panel::ALL
            .into_iter()
            .find(|&panel| side(panel) == edge && !page.panel_visible(panel) && collapsing != Some(panel));
        if claimant.is_none() && !page.split_available {
            continue;
        }
        let id = SharedString::from(format!("dock-edge-{}", edge.as_str()));
        let pulling = dragging.is_some_and(|drag| drag.side == edge && drag.gesture == Gesture::Open);
        let strip = dock::place_on_edge(dock::strip(id, edge, pulling), edge, dock::EDGE).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                cx.stop_propagation();
                match claimant {
                    // A double click opens it at the size it had.
                    Some(panel) if event.click_count >= 2 => {
                        this.set_panel_visible(panel, true);
                        this.dock_drag = None;
                    }
                    _ => this.start_edge_pull(claimant, edge, event.position),
                }
                cx.notify();
            }),
        );
        edges.push(strip.into_any_element());
    }

    // What the drag under way would do, drawn over everything.
    let guide = dragging.and_then(|drag| match (drag.gesture, drag.panel) {
        (Gesture::Move, Some(panel)) => {
            let target = dock::nearest_side(bounds, drag.pointer);
            let (min, _) = panel.range(target.is_column());
            let room = if target.is_column() { bounds.size.width } else { bounds.size.height };
            let depth = panel_size_on(panel, target).min(f32::from(room) * 0.45).max(min.min(f32::from(room)));
            let landing = dock::landing(target, depth);
            let landing = if target.is_column() { landing } else { landing.left(px(left_room)).right(px(right_room)) };
            Some(landing.into_any_element())
        }
        (Gesture::Open, None) => {
            let depth = dock::depth(drag.side, bounds, drag.pointer).max(0.0);
            Some(dock::split_guide(drag.side, depth).into_any_element())
        }
        _ => None,
    });

    div()
        .relative()
        .flex()
        .flex_row()
        .flex_grow()
        .min_h(px(0.0))
        .min_w(px(0.0))
        .children(left)
        .child(center)
        .children(right.into_iter().rev())
        .child(dock::measure(page.columns_bounds.clone()))
        .children(edges)
        .children(guide)
        .into_any_element()
}

/// The size `panel` would have docked on `side`.
fn panel_size_on(panel: Panel, side: DockSide) -> f32 {
    ARRANGEMENT.get().sizes[panel.index()][slot(side)]
}

fn side_index(side: DockSide) -> usize {
    match side {
        DockSide::Left => 0,
        DockSide::Right => 1,
        DockSide::Top => 2,
        DockSide::Bottom => 3,
    }
}

// The strip over a panel's inner edge: drag to resize (past half its
// smallest size it hides), double click for the default size, and in its
// middle the grip that carries the panel to another side.
fn edge_strip(
    page: &ExplorerPane,
    panel: Panel,
    side: DockSide,
    held: bool,
    cx: &mut Context<ExplorerPane>,
) -> impl IntoElement {
    let id = SharedString::from(format!("dock-strip-{}", panel.config_key()));
    let carrying = page
        .dock_drag
        .is_some_and(|drag| drag.panel == Some(panel) && drag.gesture == Gesture::Move);
    let grip = dock::grip(SharedString::from(format!("dock-grip-{}", panel.config_key())), side)
        .when(carrying, |grip| grip.bg(gpui::rgb(nohrs_ui::theme::theme::accent())))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                cx.stop_propagation();
                this.start_panel_move(panel, event.position);
                cx.notify();
            }),
        );
    dock::place_on_inner_edge(dock::strip(id, side, held), side)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                cx.stop_propagation();
                if event.click_count >= 2 {
                    this.reset_panel_size(panel);
                    this.dock_drag = None;
                } else {
                    this.start_panel_resize(panel, event.position);
                }
                cx.notify();
            }),
        )
        .child(grip)
}

#[cfg(test)]
mod tests {
    use super::{Panel, set_sides, set_size, size};
    use nohrs_core::config::DockSide;

    #[test]
    fn a_panel_keeps_a_size_per_kind_of_side() {
        set_sides(DockSide::Left, DockSide::Right);
        set_size(Panel::Preview, 400.0);
        set_sides(DockSide::Left, DockSide::Bottom);
        set_size(Panel::Preview, 200.0);
        set_sides(DockSide::Left, DockSide::Right);
        assert_eq!(size(Panel::Preview), 400.0);
        set_sides(DockSide::Left, DockSide::Top);
        assert_eq!(size(Panel::Preview), 200.0);
        set_sides(DockSide::Left, DockSide::Right);
    }
}
