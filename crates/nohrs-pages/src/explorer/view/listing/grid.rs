use super::truncate_middle;
use crate::explorer::{ExplorerPane, zoom};
use gpui::prelude::*;
use gpui::*;
use gpui_component::Sizable as _;
use gpui_component::input::Input;
use gpui_component::v_virtual_list;
use gpui_component::{Icon, IconName};
use nohrs_services::fs::listing::FileEntryDto;
use nohrs_ui::theme::theme;
use std::rc::Rc;

/// Room around the thumbnail inside a tile, and between tiles.
const TILE_PADDING: f32 = 10.0;
const TILE_GAP: f32 = 8.0;
/// Line heights of the name (`text_sm`) and meta (`text_xs`) lines. Fixed and
/// unshrinkable: a tile is a fixed height, and when the column ran short flex
/// took it out of the name line, clipping its descenders.
const NAME_LINE_HEIGHT: f32 = 20.0;
const META_LINE_HEIGHT: f32 = 16.0;
/// The `gap_1` the tile puts between thumbnail, name and meta.
const TILE_ROW_GAP: f32 = 4.0;
/// Everything under the thumbnail: both lines and the two gaps above them.
const TILE_LABEL_HEIGHT: f32 = NAME_LINE_HEIGHT + META_LINE_HEIGHT + TILE_ROW_GAP * 2.0;
const GRID_PADDING_X: f32 = 16.0;

/// Renders the file listing as a grid of tiles sized by the pane's zoom.
///
/// Virtualized by row: a directory of thousands of photos lays out, and asks
/// for thumbnails of, only the rows on screen.
pub fn render(
    page: &mut ExplorerPane,
    window: &mut Window,
    cx: &mut Context<ExplorerPane>,
) -> AnyElement {
    let icon = zoom::icon_size(page.zoom);
    let tile_width = icon + TILE_PADDING * 2.0;
    let tile_height = icon + TILE_PADDING * 2.0 + TILE_LABEL_HEIGHT;
    let usable = (page.grid_width - GRID_PADDING_X * 2.0).max(tile_width);
    let columns = (((usable + TILE_GAP) / (tile_width + TILE_GAP)).floor() as usize).max(1);
    page.grid_columns = columns;
    let rows = page.filtered_entries.len().div_ceil(columns);
    let thumbnail_size = zoom::thumbnail_bucket(icon, window.scale_factor());

    let row_size = size(
        px(page.grid_width.max(tile_width)),
        px(tile_height + TILE_GAP),
    );
    let row_sizes = Rc::new(vec![row_size; rows]);
    let entity = cx.entity();

    div()
        .id("grid-area")
        .relative()
        .flex_1()
        .min_h(px(0.0))
        .overflow_hidden()
        .child(
            v_virtual_list(
                entity.clone(),
                "file-grid",
                row_sizes,
                move |page, visible_rows, window, cx| {
                    visible_rows
                        .map(|row| {
                            let first = row * columns;
                            let last = (first + columns).min(page.filtered_entries.len());
                            let mut line = div()
                                .flex()
                                .flex_row()
                                .gap(px(TILE_GAP))
                                .px(px(GRID_PADDING_X))
                                .pt(px(TILE_GAP));
                            for ix in first..last {
                                let Some(item) = page.filtered_entries.get(ix).cloned() else {
                                    continue;
                                };
                                let selected = page.is_selected(ix);
                                let tile = TileSize {
                                    icon,
                                    width: tile_width,
                                    height: tile_height,
                                    thumbnail: thumbnail_size,
                                };
                                line = line.child(render_grid_item(
                                    page, item, ix, selected, tile, window, cx,
                                ));
                            }
                            line
                        })
                        .collect()
                },
            )
            .track_scroll(&page.grid_scroll_handle),
        )
        .child(
            // Measures the width rows are laid out against. A change is only
            // known after layout, so it takes effect on the next frame.
            canvas(
                move |bounds, _window, cx| {
                    let width = f32::from(bounds.size.width);
                    entity.update(cx, |page, cx| {
                        if (page.grid_width - width).abs() > 0.5 {
                            page.grid_width = width;
                            cx.notify();
                        }
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .into_any_element()
}

#[derive(Clone, Copy)]
struct TileSize {
    icon: f32,
    width: f32,
    height: f32,
    thumbnail: u32,
}

fn render_grid_item(
    page: &mut ExplorerPane,
    item: FileEntryDto,
    ix: usize,
    selected: bool,
    tile: TileSize,
    _window: &mut Window,
    cx: &mut Context<ExplorerPane>,
) -> AnyElement {
    use nohrs_ui::components::file_list::{format_date, get_file_type, human_bytes};

    let rename_input = page
        .renaming
        .as_ref()
        .filter(|state| state.index == ix)
        .map(|state| state.input.clone());

    let icon_name = match item.kind.as_str() {
        "dir" => IconName::Folder,
        _ => IconName::File,
    };

    let is_dir = item.kind == "dir";
    // Budgeted to the tile's inner width: the label is centered, so anything
    // wider is clipped at *both* ends rather than ellipsized.
    let name = truncate_middle(&item.name, ((tile.width / 7.5) as usize).max(12));
    let thumbnail = page.thumbnails.get(&item.path, tile.thumbnail, cx);
    let is_gif = item.kind == "file"
        && std::path::Path::new(&item.name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gif"));
    let animated = is_gif
        .then(|| page.gif_animation_cache(&item.path, cx))
        .flatten();
    // A folder has no meaningful byte size, so it shows its date alone rather
    // than repeating "Folder" on a second line.
    let meta_text = match item.kind.as_str() {
        "dir" => format_date(&item.modified),
        "file" => format!(
            "{} · {}",
            human_bytes(item.size),
            format_date(&item.modified)
        ),
        // Anything else (a symlink, say) shows the same user-facing label the
        // list view's Type column uses, not the raw internal kind.
        other => format!(
            "{} · {}",
            get_file_type(&item.name, other),
            format_date(&item.modified)
        ),
    };
    let activation_item = item.clone();
    let preview_item = item.clone();

    let bg_color = if selected {
        rgb(theme::accent_subtle())
    } else {
        rgb(theme::bg())
    };

    // Only the selection is outlined: a border round every tile turns a page of
    // small icons into a page of boxes.
    let border_color = if selected {
        rgb(theme::accent())
    } else {
        rgb(theme::bg())
    };

    div()
        .id(("grid-tile", ix))
        .flex_shrink_0()
        .w(px(tile.width))
        .h(px(tile.height))
        .p(px(TILE_PADDING))
        .rounded(px(8.0))
        .border_1()
        .border_color(border_color)
        .bg(bg_color)
        .when(!selected, |this| {
            this.hover(|this| this.bg(rgb(theme::bg_secondary())))
        })
        .cursor_pointer()
        .flex()
        .flex_col()
        .items_center()
        .text_center()
        .gap_1()
        .when(is_gif, |this| {
            let path = item.path.clone();
            this.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered_gif = Some(path.clone());
                } else if this.hovered_gif.as_deref() == Some(path.as_str()) {
                    this.hovered_gif = None;
                }
                cx.notify();
            }))
        })
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                this.record_click(ix, event.click_count);
                let modifiers = event.modifiers;
                if modifiers.shift {
                    this.select_range_to(ix);
                } else if modifiers.platform || modifiers.control {
                    this.toggle_select(ix);
                } else {
                    this.select_single(ix);
                }
                if preview_item.kind == "file" {
                    this.open_preview(preview_item.path.clone(), window, cx);
                }
                if event.click_count >= 2 {
                    this.activate_entry(activation_item.clone(), window, cx);
                }
                cx.notify();
            }),
        )
        .child(
            div()
                .size(px(tile.icon))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .child(match (animated, thumbnail) {
                    (Some(cache), still) => {
                        // Until the frames are decoded the still thumbnail
                        // stands in, so starting the animation is not a blink.
                        let still = still.clone();
                        img(std::path::PathBuf::from(&item.path))
                            .image_cache(&cache)
                            .size_full()
                            .object_fit(ObjectFit::Contain)
                            .with_loading(move || match &still {
                                Some(image) => img(ImageSource::Render(image.clone()))
                                    .size_full()
                                    .object_fit(ObjectFit::Contain)
                                    .into_any_element(),
                                None => div().into_any_element(),
                            })
                            .into_any_element()
                    }
                    (None, Some(image)) => img(ImageSource::Render(image))
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .into_any_element(),
                    (None, None) => Icon::new(icon_name)
                        .size(px((tile.icon * 0.6).max(24.0)))
                        .text_color(rgb(if is_dir {
                            theme::accent()
                        } else {
                            theme::gray_500()
                        }))
                        .into_any_element(),
                }),
        )
        .child(match rename_input {
            Some(input) => div()
                .w_full()
                .h(px(NAME_LINE_HEIGHT))
                .flex_shrink_0()
                .child(
                    // Matches the resting label's typography exactly, so
                    // committing the rename is not a visible jump.
                    Input::new(&input)
                        .small()
                        .appearance(false)
                        .px(px(0.0))
                        .text_sm()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(rgb(theme::fg())),
                )
                .into_any_element(),
            None => div()
                .w_full()
                .h(px(NAME_LINE_HEIGHT))
                .flex_shrink_0()
                .text_sm()
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(theme::fg()))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(name)
                .into_any_element(),
        })
        .child(
            div()
                .w_full()
                .h(px(META_LINE_HEIGHT))
                .flex_shrink_0()
                .text_xs()
                .text_color(rgb(theme::muted()))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(meta_text),
        )
        .into_any_element()
}
