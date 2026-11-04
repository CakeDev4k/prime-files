//! The explorer's three columns: sidebar, listing and preview.
//!
//! The sidebar and the preview keep the width they were dragged to and the
//! listing takes what is left, as in Explorer: widening the window widens
//! the listing. This replaced gpui-component's resizable group, which on
//! every change of the window or of a panel shared the space out again in
//! proportion and ignored the panels' limits, so the sidebar grew past its
//! maximum and the listing could be squeezed to nothing.

use std::cell::Cell;

use gpui::prelude::FluentBuilder;
use gpui::*;
use nohrs_ui::theme::theme;

use crate::explorer::ExplorerPane;

/// Widths a fresh window starts with.
const SIDEBAR_DEFAULT: f32 = 240.0;
const PREVIEW_DEFAULT: f32 = 340.0;
/// Narrowest the sidebar and the preview get, and the room always left to
/// the listing between them.
const SIDEBAR_MIN: f32 = 160.0;
const SIDEBAR_MAX: f32 = 520.0;
const PREVIEW_MIN: f32 = 240.0;
const LISTING_MIN: f32 = 220.0;
/// Width of the strip over each divider that a drag grabs.
const GRIP: f32 = 7.0;

thread_local! {
    // Shared by every tab, as the panes of Explorer's windows are: a new tab
    // opens with the widths the last drag left.
    static WIDTHS: Cell<(f32, f32)> = const { Cell::new((SIDEBAR_DEFAULT, PREVIEW_DEFAULT)) };
}

/// Which divider is being dragged.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Divider {
    /// Between the sidebar and the listing.
    Sidebar,
    /// Between the listing and the preview.
    Preview,
}

/// A drag of a divider: where it started and the width then.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DividerDrag {
    pub divider: Divider,
    pub start_x: f32,
    pub start_width: f32,
}

/// The sidebar's and the preview's widths fitted to `total`, the width of
/// the three columns: each within its range, and the listing never under
/// [`LISTING_MIN`]. The sidebar gives way first when the window is narrow.
pub(crate) fn fit_widths(sidebar: f32, preview: f32, total: f32, show_sidebar: bool, show_preview: bool) -> (f32, f32) {
    let preview = if show_preview { preview.max(PREVIEW_MIN) } else { 0.0 };
    let sidebar = if show_sidebar { sidebar.clamp(SIDEBAR_MIN, SIDEBAR_MAX) } else { 0.0 };
    if total <= 0.0 {
        return (sidebar, preview);
    }
    let preview = preview.min((total - LISTING_MIN - sidebar.min(SIDEBAR_MIN)).max(if show_preview { PREVIEW_MIN } else { 0.0 }));
    let sidebar = if show_sidebar {
        sidebar.min((total - LISTING_MIN - preview).max(SIDEBAR_MIN))
    } else {
        0.0
    };
    (sidebar, preview)
}

/// The preview's width as last laid out, in logical pixels.
pub(crate) fn preview_width() -> f32 {
    WIDTHS.get().1
}

/// The width the sidebar has when it is a column, for compact mode, where
/// it floats over the listing at that width.
pub(crate) fn sidebar_width() -> f32 {
    WIDTHS.get().0.clamp(SIDEBAR_MIN, SIDEBAR_MAX)
}

impl ExplorerPane {
    /// Starts dragging `divider` from `x`.
    pub(crate) fn start_divider_drag(&mut self, divider: Divider, x: Pixels) {
        let (sidebar, preview) = WIDTHS.get();
        self.divider_drag = Some(DividerDrag {
            divider,
            start_x: f32::from(x),
            start_width: match divider {
                Divider::Sidebar => sidebar,
                Divider::Preview => preview,
            },
        });
    }

    /// Follows a divider drag to `x`. Returns whether one is under way.
    pub(crate) fn update_divider_drag(&mut self, x: Pixels) -> bool {
        let Some(drag) = self.divider_drag else {
            return false;
        };
        let moved = f32::from(x) - drag.start_x;
        let (sidebar, preview) = WIDTHS.get();
        let total = self.columns_width.get();
        let widths = match drag.divider {
            Divider::Sidebar => (drag.start_width + moved, preview),
            // The preview's edge is on its left: dragging left widens it.
            Divider::Preview => (sidebar, drag.start_width - moved),
        };
        WIDTHS.set(fit_widths(widths.0, widths.1, total, true, true));
        true
    }

    /// Puts a divider back at its default place (a double click on it).
    pub(crate) fn reset_divider(&mut self, divider: Divider) {
        let (sidebar, preview) = WIDTHS.get();
        WIDTHS.set(match divider {
            Divider::Sidebar => (SIDEBAR_DEFAULT, preview),
            Divider::Preview => (sidebar, PREVIEW_DEFAULT),
        });
    }
}

/// The three columns, each divider with a strip to drag it by. A side that
/// is not shown (hidden, or floating in compact mode) takes no room. `amounts`
/// scale the sidebar's and the preview's widths while compact mode slides
/// them in and out as columns (1.0 otherwise); a side part way out has no
/// divider to drag.
pub(crate) fn render(
    page: &mut ExplorerPane,
    sidebar: Option<AnyElement>,
    listing: AnyElement,
    preview: Option<AnyElement>,
    amounts: (f32, f32),
    cx: &mut Context<ExplorerPane>,
) -> AnyElement {
    let (show_sidebar, show_preview) = (sidebar.is_some(), preview.is_some());
    let (sidebar_w, preview_w) = {
        let (sidebar, preview) = WIDTHS.get();
        fit_widths(sidebar, preview, page.columns_width.get(), show_sidebar, show_preview)
    };
    let (sidebar_w, preview_w) = (sidebar_w * amounts.0, preview_w * amounts.1);
    let (sidebar_grip, preview_grip) = (show_sidebar && amounts.0 >= 1.0, show_preview && amounts.1 >= 1.0);
    let measured = page.columns_width.clone();
    let dragging = page.divider_drag.map(|drag| drag.divider);
    let grip = |id: &'static str, divider: Divider, cx: &mut Context<ExplorerPane>| {
        div()
            .id(id)
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(GRIP))
            .cursor_col_resize()
            .flex()
            .justify_center()
            // The divider lights up while it is held or under the mouse.
            .child(
                div()
                    .w(px(1.0))
                    .h_full()
                    .when(dragging == Some(divider), |line| line.bg(rgb(theme::accent()))),
            )
            .hover(|style| style.bg(rgba((theme::accent() << 8) | 0x30)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                    if event.click_count >= 2 {
                        this.reset_divider(divider);
                        this.divider_drag = None;
                    } else {
                        this.start_divider_drag(divider, event.position.x);
                    }
                    cx.notify();
                }),
            )
    };
    div()
        .relative()
        .flex()
        .flex_row()
        .flex_grow()
        .min_h(px(0.0))
        .when_some(sidebar, |row, sidebar| {
            row.child(
                div()
                    .w(px(sidebar_w))
                    .flex_shrink_0()
                    .h_full()
                    .overflow_hidden()
                    .when(amounts.0 > 0.0, |column| column.border_r_1())
                    .border_color(rgb(theme::border()))
                    .child(sidebar),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .h_full()
                .flex()
                .flex_col()
                .min_h(px(0.0))
                .overflow_hidden()
                .child(listing),
        )
        .when_some(preview, |row, preview| {
            row.child(
                div()
                    .w(px(preview_w))
                    .flex_shrink_0()
                    .h_full()
                    .overflow_hidden()
                    .when(amounts.1 > 0.0, |column| column.border_l_1())
                    .border_color(rgb(theme::border()))
                    .child(preview),
            )
        })
        .child(
            canvas(
                move |bounds, window, _| {
                    let width = f32::from(bounds.size.width);
                    if (measured.replace(width) - width).abs() > 0.5 {
                        window.refresh();
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        // After the columns, so the strips are on top of both sides.
        .when(sidebar_grip, |row| {
            row.child(grip("divider-sidebar", Divider::Sidebar, cx).left(px(sidebar_w - GRIP / 2.0)))
        })
        .when(preview_grip, |row| {
            row.child(grip("divider-preview", Divider::Preview, cx).right(px(preview_w - GRIP / 2.0)))
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::fit_widths;

    #[test]
    fn the_sidebar_stays_in_its_range() {
        assert_eq!(fit_widths(900.0, 300.0, 2000.0, true, true), (520.0, 300.0));
        assert_eq!(fit_widths(10.0, 300.0, 2000.0, true, true), (160.0, 300.0));
    }

    #[test]
    fn the_listing_keeps_its_room() {
        let (sidebar, preview) = fit_widths(400.0, 1500.0, 1200.0, true, true);
        assert!(1200.0 - sidebar - preview >= 220.0, "{sidebar} {preview}");
        assert!(sidebar >= 160.0 && preview >= 240.0);
    }

    #[test]
    fn a_hidden_column_takes_no_room() {
        assert_eq!(fit_widths(300.0, 300.0, 1000.0, false, true).0, 0.0);
        assert_eq!(fit_widths(300.0, 300.0, 1000.0, true, false).1, 0.0);
    }
}
