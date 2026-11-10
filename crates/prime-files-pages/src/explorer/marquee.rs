//! Selecting by dragging a rectangle over the listing, as Explorer does: press
//! on empty space (in Details, anywhere right of the Name column) and drag;
//! every entry the rectangle touches is selected. Ctrl inverts what it touches,
//! Shift adds to the selection, and holding the pointer near (or past) the
//! listing's edge scrolls it.
//!
//! The hit test reads where each entry was actually painted rather than
//! recomputing the list, grid and List-view layouts here: those already
//! differ in three ways and a fourth copy would drift. Positions are kept in
//! content space (window position minus the scroll offset), so entries that
//! scrolled out of view during the drag still count.

use gpui::{
    Bounds, Context, IntoElement, Modifiers, ParentElement, Pixels, Point, Styled, Window, canvas, div,
    point, px, rgba,
};
use prime_files_ui::theme::theme;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

use super::ExplorerPane;
use super::types::ViewMode;

/// How far the pointer moves before a press becomes a rectangle.
const START_THRESHOLD: f32 = 4.0;
/// Band along the listing's edge where the pointer starts scrolling it.
const EDGE: f32 = 28.0;
/// Scroll speed, in pixels per second per pixel into or past that band.
const SPEED: f32 = 18.0;
/// Width of the scrollbar strip, where a press is the scrollbar's.
const SCROLLBAR: f32 = 16.0;

/// Where each entry was painted, shared with the elements that paint them.
#[derive(Clone, Default)]
pub(crate) struct HitBoxes(Rc<RefCell<HitInner>>);

#[derive(Default)]
struct HitInner {
    /// What the boxes were painted for; any change starts them over.
    key: Option<HitKey>,
    /// Entry index -> bounds in content space.
    boxes: HashMap<usize, Bounds<Pixels>>,
    /// The part of an entry that picks it up (icon and name of a line), in
    /// content space; a press beside it draws a rectangle. Only the
    /// preview's listing records these: the listing's rows know their Name
    /// column instead.
    grabs: HashMap<usize, Bounds<Pixels>>,
    /// The listing panel in window space, at the last paint.
    listing: Option<Bounds<Pixels>>,
}

type HitKey = (u8, u64, usize, u32, u32, u32);

impl HitBoxes {
    /// Forgets every box once the layout they belong to is gone.
    fn sync(&self, key: HitKey) {
        let mut inner = self.0.borrow_mut();
        if inner.key != Some(key) {
            inner.key = Some(key);
            inner.boxes.clear();
            inner.grabs.clear();
        }
    }

    // Records where entry `ix` (or the part that grabs it) was painted.
    fn track<E: ParentElement>(&self, element: E, scroll: gpui::ScrollHandle, ix: usize, grab: bool) -> E {
        let boxes = self.clone();
        element.child(
            canvas(
                move |bounds, _, _| {
                    let content = Bounds::new(bounds.origin - scroll.offset(), bounds.size);
                    let mut inner = boxes.0.borrow_mut();
                    if grab { inner.grabs.insert(ix, content) } else { inner.boxes.insert(ix, content) };
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
    }

    // The entries a rectangle in content space touches, in order.
    fn touched(&self, rect: Bounds<Pixels>) -> Vec<usize> {
        let mut hits: Vec<usize> = self
            .0
            .borrow()
            .boxes
            .iter()
            .filter(|(_, bounds)| bounds.intersects(&rect))
            .map(|(&ix, _)| ix)
            .collect();
        hits.sort_unstable();
        hits
    }

    fn listing(&self) -> Option<Bounds<Pixels>> {
        self.0.borrow().listing
    }

    pub(crate) fn set_listing(&self, bounds: Bounds<Pixels>) {
        self.0.borrow_mut().listing = Some(bounds);
    }

    fn get(&self, ix: usize) -> Option<Bounds<Pixels>> {
        self.0.borrow().boxes.get(&ix).copied()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A plain drag: the rectangle is the selection.
    Replace,
    /// Shift: the rectangle adds to what was selected.
    Add,
    /// Ctrl: the rectangle inverts what it touches.
    Toggle,
}

/// A rectangle being dragged.
pub(crate) struct Marquee<Base = HashSet<String>> {
    /// Where the press was, in content space.
    origin: Point<Pixels>,
    /// Where the press was, in window space (for the start threshold).
    press: Point<Pixels>,
    /// The pointer, in window space.
    pointer: Point<Pixels>,
    /// The selection before the drag, kept for Shift and Ctrl.
    base: Base,
    mode: Mode,
    /// Past the threshold: a rectangle, not a click.
    moved: bool,
    last_tick: Option<Instant>,
    /// The button held. With the right one, as in Explorer, the rectangle
    /// ends in the menu for what it selected (issue #37).
    button: gpui::MouseButton,
}

impl ExplorerPane {
    /// Called at the start of every listing render: drops the boxes of a
    /// layout that changed (folder, view, zoom, width).
    pub(crate) fn sync_hit_boxes(&self) {
        let mode = match self.view_mode {
            ViewMode::List => 0,
            ViewMode::Tree => 1,
            ViewMode::Grid => 2,
            ViewMode::Compact => 3,
        };
        self.hit_boxes.sync((
            mode,
            self.rows_generation,
            self.filtered_entries.len(),
            self.zoom.to_bits(),
            f32::from(self.listing_extent.width).to_bits(),
            self.grid_columns as u32,
        ));
    }

    /// Wraps an entry's element so its painted bounds are known to the
    /// rectangle. The element must be `relative()`.
    pub(crate) fn track_hit<E: ParentElement>(&self, element: E, ix: usize) -> E {
        self.hit_boxes.track(element, self.listing_scroll().clone(), ix, false)
    }

    /// Whether a press at `position` on row `ix` of the Details view is on
    /// the row's blank part (right of the Name column), where Explorer starts
    /// a rectangle instead of picking up the file.
    pub(crate) fn press_on_row_blank(&self, ix: usize, position: Point<Pixels>) -> bool {
        let Some(content) = self.hit_boxes.get(ix) else {
            return false;
        };
        let left = content.origin.x + self.listing_scroll().offset().x;
        position.x > left + px(super::view::listing::list::ROW_MARGIN + self.col_name_width)
    }

    /// Starts a rectangle at `position` (window space). `on_row` is the row
    /// whose blank part was pressed, if any. Returns whether one started.
    pub(crate) fn start_marquee(
        &mut self,
        position: Point<Pixels>,
        modifiers: Modifiers,
        on_row: Option<usize>,
        cx: &mut Context<Self>,
    ) -> bool {
        self.start_marquee_with(position, modifiers, on_row, gpui::MouseButton::Left, cx)
    }

    /// The same, held with `button`.
    pub(crate) fn start_marquee_with(
        &mut self,
        position: Point<Pixels>,
        modifiers: Modifiers,
        on_row: Option<usize>,
        button: gpui::MouseButton,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.renaming.is_some() {
            return false;
        }
        // The scrollbar sits over the listing's far edge and takes its own
        // presses; a rectangle started there would fight it.
        if let Some(listing) = self.hit_boxes.listing() {
            let along_bar = if self.view_mode == ViewMode::Compact {
                position.y > listing.bottom() - px(SCROLLBAR)
            } else {
                position.x > listing.right() - px(SCROLLBAR)
            };
            if !listing.contains(&position) || along_bar {
                return false;
            }
        }
        let mode = Mode::of(modifiers);
        let base = if mode == Mode::Replace { HashSet::new() } else { self.selection.clone() };
        // A click on empty space clears the selection at once, as in
        // Explorer; on a row's blank part it stays that row's click until the
        // pointer moves.
        if mode == Mode::Replace && on_row.is_none() && !self.selection.is_empty() {
            self.clear_selection();
            cx.notify();
        }
        self.drag_candidate = None;
        let mut marquee = Marquee::new(position, self.listing_scroll().offset(), base, mode);
        marquee.button = button;
        self.marquee = Some(marquee);
        true
    }

    /// The button the rectangle is held with, if one is under way.
    pub(crate) fn marquee_button(&self) -> Option<gpui::MouseButton> {
        self.marquee.as_ref().map(|marquee| marquee.button)
    }

    /// A right press on empty space was let go: the menu for what the
    /// rectangle selected, or the folder's menu when it was only a click or
    /// selected nothing.
    pub(crate) fn finish_right_marquee(&mut self, extended: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(marquee) = self.marquee.take() else {
            return;
        };
        let row = if marquee.moved && !self.selection.is_empty() {
            self.active_row().or_else(|| (0..self.filtered_entries.len()).find(|&ix| self.is_selected(ix)))
        } else {
            None
        };
        cx.notify();
        self.open_context_menu(row, extended, window, cx);
    }

    /// Whether a rectangle is being dragged (so a row's click is not a click).
    pub(crate) fn marquee_dragging(&self) -> bool {
        self.marquee.as_ref().is_some_and(|marquee| marquee.moved)
    }

    /// Follows the pointer. `pressed` is whether the left button is still
    /// down: a release that never reached us ends the rectangle here.
    pub(crate) fn marquee_moved(&mut self, position: Point<Pixels>, pressed: bool, cx: &mut Context<Self>) {
        let Some(marquee) = self.marquee.as_mut() else {
            return;
        };
        if !pressed {
            self.end_marquee(cx);
            return;
        }
        if !marquee.follow(position) {
            return;
        }
        self.apply_marquee();
        cx.notify();
    }

    /// Ends the rectangle; the selection it made stays.
    pub(crate) fn end_marquee(&mut self, cx: &mut Context<Self>) {
        if let Some(marquee) = self.marquee.take()
            && marquee.moved
        {
            cx.notify();
        }
    }

    // The rectangle in content space.
    fn marquee_rect(&self, marquee: &Marquee) -> Bounds<Pixels> {
        marquee.rect(self.listing_scroll().offset())
    }

    // Sets the selection from what the rectangle touches.
    fn apply_marquee(&mut self) {
        let Some(marquee) = self.marquee.as_ref() else {
            return;
        };
        let hits = self.hit_boxes.touched(self.marquee_rect(marquee));
        let hit_paths: Vec<String> = hits
            .iter()
            .filter_map(|&ix| self.filtered_entries.get(ix).map(|entry| entry.path.clone()))
            .collect();
        let mut selection = marquee.base.clone();
        for path in &hit_paths {
            match marquee.mode {
                Mode::Replace | Mode::Add => {
                    selection.insert(path.clone());
                }
                Mode::Toggle => {
                    if !selection.remove(path) {
                        selection.insert(path.clone());
                    }
                }
            }
        }
        self.selection = selection;
        if let (Some(first), Some(last)) = (hit_paths.first(), hit_paths.last()) {
            self.selection_anchor = Some(first.clone());
            self.active_path = Some(last.clone());
        }
    }

    /// Scrolls the listing while the pointer is held at or past its edge;
    /// called on every render, since the pointer may sit still meanwhile.
    pub(crate) fn tick_marquee(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(listing) = self.hit_boxes.listing() else {
            return;
        };
        let horizontal = self.view_mode == ViewMode::Compact;
        let Some(marquee) = self.marquee.as_mut() else {
            return;
        };
        if !marquee.moved {
            return;
        }
        let Some(step) = marquee.edge_step(window.mouse_position(), listing, horizontal) else {
            return;
        };
        let handle = self.listing_scroll().clone();
        let max = handle.max_offset();
        let mut offset = handle.offset();
        if horizontal {
            offset.x = (offset.x - px(step)).clamp(-max.width, px(0.0));
        } else {
            offset.y = (offset.y - px(step)).clamp(-max.height, px(0.0));
        }
        if offset != handle.offset() {
            self.smooth_scroll = None;
            handle.set_offset(offset);
            self.apply_marquee();
            cx.notify();
        }
        window.request_animation_frame();
    }

    /// The rectangle as drawn over the listing, if one is being dragged.
    pub(crate) fn marquee_overlay(&self) -> Option<gpui::AnyElement> {
        let marquee = self.marquee.as_ref().filter(|marquee| marquee.moved)?;
        let listing = self.hit_boxes.listing()?;
        let offset = self.listing_scroll().offset();
        overlay(self.marquee_rect(marquee), offset, listing)
    }
}

impl Mode {
    fn of(modifiers: Modifiers) -> Self {
        if modifiers.shift {
            Mode::Add
        } else if modifiers.control || modifiers.platform {
            Mode::Toggle
        } else {
            Mode::Replace
        }
    }
}

impl<Base> Marquee<Base> {
    fn new(position: Point<Pixels>, scroll: Point<Pixels>, base: Base, mode: Mode) -> Self {
        Self {
            origin: position - scroll,
            press: position,
            pointer: position,
            base,
            mode,
            moved: false,
            last_tick: None,
            button: gpui::MouseButton::Left,
        }
    }

    // Takes the pointer; whether the press has become a rectangle.
    fn follow(&mut self, position: Point<Pixels>) -> bool {
        self.pointer = position;
        if !self.moved {
            let moved = position - self.press;
            if f32::from(moved.x).abs() < START_THRESHOLD && f32::from(moved.y).abs() < START_THRESHOLD {
                return false;
            }
            self.moved = true;
        }
        true
    }

    // The rectangle in content space, for a listing scrolled by `scroll`.
    fn rect(&self, scroll: Point<Pixels>) -> Bounds<Pixels> {
        let current = self.pointer - scroll;
        Bounds::from_corners(
            point(self.origin.x.min(current.x), self.origin.y.min(current.y)),
            point(self.origin.x.max(current.x), self.origin.y.max(current.y)),
        )
    }

    // How far to scroll this frame with the pointer at `pointer`: some only
    // while it is held in or past the band along the listing's edge.
    fn edge_step(&mut self, pointer: Point<Pixels>, listing: Bounds<Pixels>, horizontal: bool) -> Option<f32> {
        self.pointer = pointer;
        let (pos, low, high) = if horizontal {
            (pointer.x, listing.left(), listing.right())
        } else {
            (pointer.y, listing.top(), listing.bottom())
        };
        let push = if pos < low + px(EDGE) {
            f32::from(pos - (low + px(EDGE)))
        } else if pos > high - px(EDGE) {
            f32::from(pos - (high - px(EDGE)))
        } else {
            self.last_tick = None;
            return None;
        };
        let now = Instant::now();
        let elapsed = self
            .last_tick
            .map_or(1.0 / 60.0, |last| now.duration_since(last).as_secs_f32())
            .min(0.1);
        self.last_tick = Some(now);
        Some(push * SPEED * elapsed)
    }
}

// The rectangle (content space) drawn over a listing scrolled by `scroll`,
// cut to the listing's bounds.
fn overlay(rect: Bounds<Pixels>, scroll: Point<Pixels>, listing: Bounds<Pixels>) -> Option<gpui::AnyElement> {
    let a = rect.origin + scroll;
    let b = rect.bottom_right() + scroll;
    let left = a.x.max(listing.left());
    let top = a.y.max(listing.top());
    let right = b.x.min(listing.right());
    let bottom = b.y.min(listing.bottom());
    if right <= left || bottom <= top {
        return None;
    }
    Some(
        div()
            .absolute()
            .left(left - listing.left())
            .top(top - listing.top())
            .w(right - left)
            .h(bottom - top)
            .bg(rgba((theme::accent() << 8) | 0x33))
            .border_1()
            .border_color(rgba((theme::accent() << 8) | 0xcc))
            .into_any_element(),
    )
}

/// The rectangle over the preview's listing of a folder: the same gesture,
/// over the items the preview lists, picking them for the drag out.
pub(crate) type PreviewMarquee = Marquee<BTreeSet<usize>>;

impl ExplorerPane {
    /// Called as the preview's listing renders: drops the boxes of a layout
    /// that changed (another folder, lines or grid, zoom, width).
    pub(crate) fn sync_preview_hit_boxes(&self, grid: bool, columns: usize, width: f32) {
        let (key, len) = self
            .preview_listing
            .as_ref()
            .map_or((0, 0), |listing| (std::rc::Rc::as_ptr(listing) as u64, listing.items.len()));
        self.preview_hit_boxes.sync((
            u8::from(grid),
            key,
            len,
            self.preview_listing_zoom.to_bits(),
            width.to_bits(),
            columns as u32,
        ));
    }

    /// Records where item `ix` of the preview's listing was painted. The
    /// element must be `relative()`.
    pub(crate) fn track_preview_hit<E: ParentElement>(&self, element: E, ix: usize) -> E {
        self.preview_hit_boxes.track(element, (*self.preview_listing_scroll).clone(), ix, false)
    }

    /// Records the part of line `ix` that picks the item up (its icon and
    /// name). The element must be `relative()`.
    pub(crate) fn track_preview_grab<E: ParentElement>(&self, element: E, ix: usize) -> E {
        self.preview_hit_boxes.track(element, (*self.preview_listing_scroll).clone(), ix, true)
    }

    /// Where the preview's listing was painted, and item `ix` in it (content
    /// space), for the tests that drag over it.
    #[cfg(test)]
    pub(crate) fn preview_listing_painted(&self, ix: usize) -> (Option<Bounds<Pixels>>, Option<Bounds<Pixels>>) {
        (self.preview_hit_boxes.listing(), self.preview_hit_boxes.get(ix))
    }

    pub(crate) fn set_preview_listing_bounds(&self, bounds: Bounds<Pixels>) {
        self.preview_hit_boxes.set_listing(bounds);
    }

    /// Whether a press at `position` on line `ix` is beside its icon and
    /// name, where Explorer draws a rectangle instead of picking the file up.
    pub(crate) fn press_beside_preview_item(&self, ix: usize, position: Point<Pixels>) -> bool {
        let Some(grab) = self.preview_hit_boxes.0.borrow().grabs.get(&ix).copied() else {
            return false;
        };
        let content = position - self.preview_listing_scroll.offset();
        !grab.contains(&content)
    }

    /// Starts a rectangle over the preview's listing at `position`. `on_item`
    /// is the line whose blank part was pressed, which stays that line's
    /// click until the pointer moves.
    pub(crate) fn start_preview_marquee(
        &mut self,
        position: Point<Pixels>,
        modifiers: Modifiers,
        on_item: Option<usize>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(listing) = self.preview_listing.clone() else {
            return false;
        };
        if self.preview_hit_boxes.listing().is_some_and(|bounds| !bounds.contains(&position)) {
            return false;
        }
        let mode = Mode::of(modifiers);
        let picked = self.preview_listing_selection.picked_in(&listing);
        if mode == Mode::Replace && on_item.is_none() && !picked.is_empty() {
            self.preview_listing_selection.clear();
            cx.notify();
        }
        let base = if mode == Mode::Replace { BTreeSet::new() } else { picked };
        self.preview_drag = None;
        let scroll = self.preview_listing_scroll.offset();
        self.preview_marquee = Some(Marquee::new(position, scroll, base, mode));
        true
    }

    /// Whether a rectangle is being dragged over the preview's listing, so a
    /// line's click is not a click.
    pub(crate) fn preview_marquee_dragging(&self) -> bool {
        self.preview_marquee.as_ref().is_some_and(|marquee| marquee.moved)
    }

    pub(crate) fn preview_marquee_moved(&mut self, position: Point<Pixels>, pressed: bool, cx: &mut Context<Self>) {
        let Some(marquee) = self.preview_marquee.as_mut() else {
            return;
        };
        if !pressed {
            self.end_preview_marquee(cx);
            return;
        }
        if !marquee.follow(position) {
            return;
        }
        self.apply_preview_marquee();
        cx.notify();
    }

    pub(crate) fn end_preview_marquee(&mut self, cx: &mut Context<Self>) {
        if let Some(marquee) = self.preview_marquee.take()
            && marquee.moved
        {
            cx.notify();
        }
    }

    fn apply_preview_marquee(&mut self) {
        let (Some(marquee), Some(listing)) = (self.preview_marquee.as_ref(), self.preview_listing.clone()) else {
            return;
        };
        let hits = self
            .preview_hit_boxes
            .touched(marquee.rect(self.preview_listing_scroll.offset()));
        let picked = combine(&marquee.base, &hits, marquee.mode);
        self.preview_listing_selection.set_picked(&listing, picked, hits.first().copied());
    }

    /// Scrolls the preview's listing while the rectangle is held at or past
    /// its edge.
    pub(crate) fn tick_preview_marquee(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(listing) = self.preview_hit_boxes.listing() else {
            return;
        };
        let Some(marquee) = self.preview_marquee.as_mut().filter(|marquee| marquee.moved) else {
            return;
        };
        let Some(step) = marquee.edge_step(window.mouse_position(), listing, false) else {
            return;
        };
        let handle = (*self.preview_listing_scroll).clone();
        let max = handle.max_offset();
        let mut offset = handle.offset();
        offset.y = (offset.y - px(step)).clamp(-max.height, px(0.0));
        if offset != handle.offset() {
            handle.set_offset(offset);
            self.apply_preview_marquee();
            cx.notify();
        }
        window.request_animation_frame();
    }

    /// The rectangle over the preview's listing, if one is being dragged.
    pub(crate) fn preview_marquee_overlay(&self) -> Option<gpui::AnyElement> {
        let marquee = self.preview_marquee.as_ref().filter(|marquee| marquee.moved)?;
        let listing = self.preview_hit_boxes.listing()?;
        let scroll = self.preview_listing_scroll.offset();
        overlay(marquee.rect(scroll), scroll, listing)
    }
}

// The pick a rectangle makes: what it touches, over what was picked before
// for Shift (added) and Ctrl (inverted).
fn combine(base: &BTreeSet<usize>, hits: &[usize], mode: Mode) -> BTreeSet<usize> {
    let mut picked = base.clone();
    for &ix in hits {
        if mode == Mode::Toggle {
            if !picked.remove(&ix) {
                picked.insert(ix);
            }
        } else {
            picked.insert(ix);
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::{Mode, combine};
    use std::collections::BTreeSet;

    #[test]
    fn a_rectangle_replaces_adds_or_inverts() {
        let before: BTreeSet<usize> = [1, 2].into();
        assert_eq!(combine(&BTreeSet::new(), &[2, 3], Mode::Replace), [2, 3].into());
        assert_eq!(combine(&before, &[2, 3], Mode::Add), [1, 2, 3].into());
        assert_eq!(combine(&before, &[2, 3], Mode::Toggle), [1, 3].into());
    }
}
