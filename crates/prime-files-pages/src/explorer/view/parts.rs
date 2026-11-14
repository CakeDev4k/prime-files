//! The pane's listing, sidebar and player as views of their own, so a
//! video's frames redraw the player alone.
//!
//! gpui redraws a view when it, or a view inside it, is told something
//! changed, and every ancestor with it (`Window::mark_view_dirty`). The
//! player lived in the pane's own render, so each of the many frames a
//! second a video sends rebuilt the whole pane: the listing (every visible
//! row), the sidebar and all. Until 14/11/2026 that was ~23% of a core with a
//! video playing (`docs/PERFORMANCE.md`, passes 20 and 31).
//!
//! Now the listing and the sidebar are cached views, and the player a view
//! the media task tells of each frame (`media::redraw`) instead of the pane.
//! The listing and the sidebar read the pane, globals (tags, network places,
//! bookmarks) and the config; until now any change anywhere in the window
//! drew them again, and that stays: they are registered with the patched
//! gpui to follow every change (`Window::redraw_with_every_change`), and
//! the player to draw alone (`Window::redraw_alone`), so a frame of the
//! video is the one change they sit out (`third_party/gpui/PRIME-FILES.md`).
//! They also observe the pane, for a window that has not registered them.

/// Registers the parts with `window` (again after a tab moved windows).
pub(crate) fn register(parts: &Parts, window: &mut Window) {
    window.redraw_with_every_change(parts.listing.entity_id());
    window.redraw_with_every_change(parts.sidebar.entity_id());
    window.redraw_alone(parts.player.entity_id());
}

use gpui::*;

use crate::explorer::ExplorerPane;

/// The three views a pane keeps for itself, made on its first render.
pub(crate) struct Parts {
    pub listing: Entity<ListingPart>,
    pub sidebar: Entity<SidebarPart>,
    pub player: Entity<PlayerPart>,
}

impl Parts {
    pub(crate) fn new(pane: &Entity<ExplorerPane>, cx: &mut App) -> Self {
        let weak = pane.downgrade();
        Self {
            listing: cx.new(|cx| ListingPart { pane: weak.clone(), _follow: follow(pane, cx) }),
            sidebar: cx.new(|cx| SidebarPart { pane: weak.clone(), _follow: follow(pane, cx) }),
            player: cx.new(|_| PlayerPart { pane: weak }),
        }
    }
}

// Redraws the part whenever the pane says it changed.
fn follow<T: 'static>(pane: &Entity<ExplorerPane>, cx: &mut Context<T>) -> Subscription {
    cx.observe(pane, |_, _, cx| cx.notify())
}

/// The listing, as the pane's center lays it out (`panels::render`).
pub(crate) fn listing_view(parts: &Parts) -> AnyElement {
    let mut style = StyleRefinement::default();
    style.flex_grow = Some(1.0);
    style.flex_shrink = Some(1.0);
    style.flex_basis = Some(relative(0.).into());
    style.min_size.width = Some(px(0.0).into());
    style.min_size.height = Some(px(0.0).into());
    AnyView::from(parts.listing.clone()).cached(style).into_any_element()
}

/// The sidebar, filling the column `panels::render` sized for it.
pub(crate) fn sidebar_view(parts: &Parts) -> AnyElement {
    AnyView::from(parts.sidebar.clone()).cached(StyleRefinement::default().size_full()).into_any_element()
}

/// The player, where the preview shows it.
pub(crate) fn player_view(parts: &Parts) -> AnyElement {
    AnyView::from(parts.player.clone()).into_any_element()
}

// A cached view is laid out on its own, as a root of the size its box got:
// what it draws is put in a box that fills that, as the content of the
// pane's column did.
fn fill(element: AnyElement) -> AnyElement {
    div().size_full().flex().flex_col().min_h(px(0.0)).min_w(px(0.0)).child(element).into_any_element()
}

pub(crate) struct ListingPart {
    pane: WeakEntity<ExplorerPane>,
    _follow: Subscription,
}

impl Render for ListingPart {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let drawn = self.pane.update(cx, |pane, cx| super::listing::render(pane, window, cx));
        fill(drawn.unwrap_or_else(|_| div().into_any_element()))
    }
}

pub(crate) struct SidebarPart {
    pane: WeakEntity<ExplorerPane>,
    _follow: Subscription,
}

impl Render for SidebarPart {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let drawn = self.pane.update(cx, |pane, cx| super::sidebar::render(pane, window, cx).into_any_element());
        fill(drawn.unwrap_or_else(|_| div().into_any_element()))
    }
}

pub(crate) struct PlayerPart {
    pane: WeakEntity<ExplorerPane>,
}

impl Render for PlayerPart {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let drawn = self.pane.update(cx, |pane, cx| super::preview::render_media(pane, cx));
        drawn.unwrap_or_else(|_| div().into_any_element())
    }
}
