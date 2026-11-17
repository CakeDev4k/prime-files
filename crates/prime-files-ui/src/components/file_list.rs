use prime_files_core::config::DateFormat;
use prime_files_core::t;
use crate::theme::theme;
use gpui::{ParentElement, Styled, Window, div, px, rgb};
use gpui_component::list::{ListDelegate, ListItem, ListState};
use gpui_component::{Icon, IconName, IndexPath};
use prime_files_models::file_entry::FileEntryDto;
use std::sync::Arc;

/// Average glyph width used to size the compact columns without text layout,
/// the same estimate `fit_column` makes.
pub const GLYPH_WIDTH: f32 = 7.2;
/// Padding a measured column adds around its text.
pub const COLUMN_PADDING: f32 = 20.0;

/// Width of the compact Modified cell. A date fits in the width the cell has
/// always had; the date plus the hour does not, and `text_ellipsis` would cut
/// the time off.
pub fn compact_modified_width(format: DateFormat) -> f32 {
    match format {
        DateFormat::Date => 90.0,
        // "2026/11/15 18:46" is 16 glyphs.
        DateFormat::DateTime => (16.0 * GLYPH_WIDTH + COLUMN_PADDING).max(90.0),
    }
}

/// Callback invoked when a file list entry is confirmed (clicked or activated).
pub type ConfirmCallback = Arc<dyn Fn(&FileEntryDto) + 'static>;

/// List delegate that renders file entries and tracks selection for the file list view.
pub struct FileListDelegate {
    /// File entries currently displayed in the list.
    pub items: Vec<FileEntryDto>,
    /// Index path of the currently selected row, if any.
    pub selected: Option<IndexPath>,
    // Callback hooks
    /// Callback invoked when an entry is confirmed.
    pub on_confirm: Option<ConfirmCallback>,
    /// How the Modificado column writes its date.
    pub date_format: DateFormat,
}

impl Default for FileListDelegate {
    fn default() -> Self {
        Self::new()
    }
}

impl FileListDelegate {
    /// Create an empty delegate with no items, selection, or callbacks.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected: None,
            on_confirm: None,
            date_format: DateFormat::Date,
        }
    }

    /// Replace the displayed entries and clear the current selection.
    pub fn set_items(&mut self, items: Vec<FileEntryDto>) {
        self.items = items;
        self.selected = None;
    }

    /// Return the currently selected entry, if any.
    pub fn get_selected(&self) -> Option<&FileEntryDto> {
        self.selected.and_then(|ix| self.items.get(ix.row))
    }
}

impl ListDelegate for FileListDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &gpui::App) -> usize {
        self.items.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        _cx: &mut gpui::Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let item = self.items.get(ix.row)?;

        // Icon based on file type
        let icon_name = match item.kind.as_str() {
            "dir" => IconName::Folder,
            _ => IconName::File,
        };

        // Alternate row background for zebra striping
        let bg_color = if ix.row.is_multiple_of(2) {
            theme::bg()
        } else {
            theme::gray_50()
        };

        let file_type = get_file_type(&item.name, &item.kind);

        let mut row = ListItem::new(ix)
            .py(px(6.0)) // Reduced from 12.0 for compact rows
            .px(px(24.0))
            .bg(rgb(bg_color))
            .child(
                div()
                    .flex()
                    .items_center()
                    .w_full()
                    .child(
                        // Name column with icon - flexible, takes remaining space
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .flex_1()
                            .min_w(px(150.0))
                            .child(
                                Icon::new(icon_name)
                                    .size_4()
                                    .text_color(rgb(theme::gray_600())),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(rgb(theme::fg()))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(item.name.clone()),
                            ),
                    )
                    .child(
                        // Type column - compact
                        div()
                            .w(px(70.0))
                            .flex_shrink_0()
                            .text_sm()
                            .text_color(rgb(theme::fg_secondary()))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(file_type),
                    )
                    .child(
                        // Size column - compact
                        div()
                            .w(px(70.0))
                            .flex_shrink_0()
                            .text_sm()
                            .text_color(rgb(theme::fg_secondary()))
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(match item.kind.as_str() {
                                "file" => human_bytes(item.size),
                                "dir" => "-".to_string(),
                                other => other.to_string(),
                            }),
                    )
                    .child(
                        // Modified column - compact
                        div()
                            .w(px(compact_modified_width(self.date_format)))
                            .flex_shrink_0()
                            .text_sm()
                            .text_color(rgb(theme::fg_secondary()))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(format_date(&item.modified, self.date_format)),
                    )
                    .child(
                        // Actions column - compact
                        div()
                            .w(px(40.0))
                            .flex_shrink_0()
                            .flex()
                            .justify_end()
                            .child(
                                Icon::new(IconName::File)
                                    .size_4()
                                    .text_color(rgb(theme::muted()))
                                    .cursor_pointer(),
                            ),
                    ),
            );

        // enable click to confirm
        if let Some(on_confirm) = self.on_confirm.as_ref() {
            let on_confirm = Arc::clone(on_confirm);
            let item_clone = item.clone();
            row = row.on_click(move |_, _, _| {
                on_confirm(&item_clone);
            });
        }
        Some(row)
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        _cx: &mut gpui::Context<ListState<Self>>,
    ) {
        self.selected = ix;
    }

    fn confirm(
        &mut self,
        _secondary: bool,
        _window: &mut Window,
        _cx: &mut gpui::Context<ListState<Self>>,
    ) {
        if let Some(ix) = self.selected
            && let Some(item) = self.items.get(ix.row)
            && let Some(cb) = &self.on_confirm
        {
            cb(item);
        }
    }
}

/// Format a byte count as a human-readable size string (B, KB, MB, or GB).
pub fn human_bytes(size: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * KB;
    const GB: f64 = 1024.0 * MB;
    let s = size as f64;
    if s >= GB {
        format!("{:.1} GB", s / GB)
    } else if s >= MB {
        format!("{:.1} MB", s / MB)
    } else if s >= KB {
        format!("{:.1} KB", s / KB)
    } else {
        format!("{} B", size)
    }
}

/// Format a Unix timestamp (seconds) for the Modificado column: the day, or the
/// day and the hour, as `date_format` asks. `-` when the stamp is invalid.
pub fn format_date(timestamp: &u64, date_format: DateFormat) -> String {
    use time::OffsetDateTime;
    use time::UtcOffset;
    use time::macros::format_description;

    let format = match date_format {
        DateFormat::Date => format_description!("[year]/[month]/[day]"),
        DateFormat::DateTime => format_description!("[year]/[month]/[day] [hour]:[minute]"),
    };
    match OffsetDateTime::from_unix_timestamp(*timestamp as i64) {
        // Group headers use the local offset, so the hour here has to be the
        // local hour too, taken at that moment (a file from winter shows the
        // winter hour, not today's). When the zone cannot be read, stay in UTC.
        Ok(datetime) => {
            // `to_offset` panics when the shift goes past the year 9999 limit
            // (a stamp of 253402300799 is 9999-12-31). Checked: stay in UTC.
            let offset = UtcOffset::local_offset_at(datetime).unwrap_or(UtcOffset::UTC);
            let shifted = datetime.checked_to_offset(offset).unwrap_or(datetime);
            shifted.format(&format).unwrap_or_else(|_| "-".to_string())
        }
        Err(_) => "-".to_string(),
    }
}

/// Derive a human-readable type label from an entry's name and kind (e.g. `Folder`, `PNG`, `Link`).
pub fn get_file_type(name: &str, kind: &str) -> String {
    match kind {
        "dir" => t!("Pasta").to_string(),
        "file" => {
            if let Some(ext) = std::path::Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
            {
                ext.to_uppercase()
            } else {
                t!("Arquivo").to_string()
            }
        }
        "symlink" => t!("Atalho").to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COLUMN_PADDING, GLYPH_WIDTH, FileListDelegate, compact_modified_width, format_date,
        get_file_type, human_bytes,
    };
    use prime_files_core::config::DateFormat;
    use gpui::TestAppContext;
    use gpui_component::IndexPath;
    use gpui_component::list::{ListDelegate, ListState};
    use prime_files_models::file_entry::FileEntryDto;
    use proptest::prelude::*;
    use std::sync::{Arc, Mutex};

    fn entry(name: &str) -> FileEntryDto {
        FileEntryDto {
            name: name.into(),
            path: format!("/{name}"),
            kind: "file".into(),
            size: 0,
            modified: 0,
            created: 0,
            attributes: 0,
        }
    }

    fn dir_entry(name: &str) -> FileEntryDto {
        FileEntryDto {
            kind: "dir".into(),
            ..entry(name)
        }
    }

    #[test]
    fn delegate_starts_empty() {
        let delegate = FileListDelegate::new();
        assert!(delegate.items.is_empty());
        assert!(delegate.get_selected().is_none());
    }

    #[test]
    fn set_items_replaces_items_and_clears_selection() {
        let mut delegate = FileListDelegate::new();
        delegate.selected = Some(IndexPath::new(0));
        delegate.set_items(vec![entry("a"), entry("b")]);
        assert_eq!(delegate.items.len(), 2);
        assert!(
            delegate.get_selected().is_none(),
            "set_items resets the selection"
        );
    }

    #[test]
    fn get_selected_resolves_the_selected_row() {
        let mut delegate = FileListDelegate::new();
        delegate.set_items(vec![entry("a"), entry("b")]);
        delegate.selected = Some(IndexPath::new(1));
        assert_eq!(delegate.get_selected().map(|e| e.name.as_str()), Some("b"));
    }

    #[test]
    fn human_bytes_picks_the_expected_unit() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(human_bytes(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn get_file_type_maps_kinds_and_extensions() {
        assert_eq!(get_file_type("photo.png", "file"), "PNG");
        assert_eq!(get_file_type("Makefile", "file"), "Arquivo");
        assert_eq!(get_file_type("src", "dir"), "Pasta");
        assert_eq!(get_file_type("link", "symlink"), "Atalho");
    }

    #[test]
    fn format_date_is_stable_for_the_epoch() {
        // Both formats are checked against the offset at that moment, so a
        // machine in another zone still passes.
        use time::{OffsetDateTime, UtcOffset, macros::format_description};
        let epoch = OffsetDateTime::from_unix_timestamp(0).unwrap();
        let offset = UtcOffset::local_offset_at(epoch).unwrap_or(UtcOffset::UTC);
        let local = epoch.to_offset(offset);
        assert_eq!(
            format_date(&0, DateFormat::Date),
            local.format(&format_description!("[year]/[month]/[day]")).unwrap()
        );
        assert_eq!(
            format_date(&0, DateFormat::DateTime),
            local
                .format(&format_description!("[year]/[month]/[day] [hour]:[minute]"))
                .unwrap()
        );
    }

    #[test]
    fn the_last_day_of_the_calendar_does_not_panic_on_a_positive_offset() {
        use time::{OffsetDateTime, UtcOffset, macros::format_description};
        // 9999-12-31 23:59:59 UTC is the last instant the type can hold, so any
        // positive offset pushes it past the limit. `to_offset` panicked there;
        // the checked one says no and the formatter stays in UTC.
        let stamp = 253402300799u64;
        let last_day = OffsetDateTime::from_unix_timestamp(stamp as i64).unwrap();
        let east_of_utc = UtcOffset::from_hms(14, 0, 0).unwrap();
        assert!(last_day.checked_to_offset(east_of_utc).is_none(), "the shift is the overflow");

        // Lisbon is UTC at this date, so the hour shown is the UTC hour. If the
        // machine sits east of UTC the shift is applied where it is still possible.
        let offset = UtcOffset::local_offset_at(last_day).unwrap_or(UtcOffset::UTC);
        let shown = last_day.checked_to_offset(offset).unwrap_or(last_day);
        assert_eq!(
            format_date(&stamp, DateFormat::DateTime),
            shown
                .format(&format_description!("[year]/[month]/[day] [hour]:[minute]"))
                .unwrap(),
            "the formatter answered instead of panicking"
        );
        assert_ne!(format_date(&stamp, DateFormat::DateTime), "-");
    }

    #[test]
    fn the_compact_modified_cell_fits_the_time() {
        // Measured from what `format_date` really renders, not from a guess.
        let glyphs = format_date(&0, DateFormat::DateTime).chars().count();
        assert_eq!(glyphs, 16);
        assert_eq!(compact_modified_width(DateFormat::Date), 90.0);
        assert!(
            compact_modified_width(DateFormat::DateTime)
                >= glyphs as f32 * GLYPH_WIDTH + COLUMN_PADDING,
            "the hour and minute would be ellipsed"
        );
    }

    proptest! {
        // A representative property: the size formatter must never panic and
        // must always emit a non-empty string for any `u64` input.
        #[test]
        fn human_bytes_never_panics(size in any::<u64>()) {
            let rendered = human_bytes(size);
            prop_assert!(!rendered.is_empty());
        }
    }

    // GPUI tests: drive the delegate through a real `ListState<FileListDelegate>`
    // entity in a test window so the `ListDelegate` impl (including the
    // `render_item` element tree painted for the visible rows) actually runs.

    #[gpui::test]
    async fn list_renders_rows_and_tracks_selection(cx: &mut TestAppContext) {
        // gpui-component installs the `Theme` global the list/icon widgets read
        // while painting; without it the row icons would panic during the draw.
        cx.update(gpui_component::init);
        let (list, cx) = cx.add_window_view(|window, cx| {
            let mut delegate = FileListDelegate::new();
            // A file, a directory, and an extensioned file exercise every arm of
            // `render_item` (icon choice, size column, type column).
            delegate.set_items(vec![entry("a.txt"), dir_entry("src"), entry("b.png")]);
            ListState::new(delegate, window, cx)
        });
        // The maximized test window paints the rows on open.
        cx.run_until_parked();

        list.read_with(cx, |list, cx| {
            assert_eq!(list.delegate().items_count(0, cx), 3);
            assert!(list.selected_index().is_none());
        });

        // Selecting routes through the delegate and is resolvable afterwards.
        list.update_in(cx, |list, window, cx| {
            list.set_selected_index(Some(IndexPath::new(1)), window, cx);
        });
        list.read_with(cx, |list, _cx| {
            assert_eq!(
                list.delegate().get_selected().map(|e| e.name.as_str()),
                Some("src")
            );
        });
    }

    #[gpui::test]
    async fn confirm_invokes_callback_for_selected_item(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        // `ConfirmCallback` is `Arc<dyn Fn + 'static>`; an `Arc<Mutex<_>>` sink
        // keeps the closure `Send + Sync` (clippy::arc_with_non_send_sync).
        let confirmed: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let (list, cx) = cx.add_window_view(|window, cx| {
            let mut delegate = FileListDelegate::new();
            delegate.set_items(vec![entry("a.txt"), entry("b.txt")]);
            let sink = Arc::clone(&confirmed);
            delegate.on_confirm = Some(Arc::new(move |item: &FileEntryDto| {
                *sink.lock().unwrap() = Some(item.name.clone());
            }));
            ListState::new(delegate, window, cx)
        });

        list.update_in(cx, |list, window, cx| {
            list.set_selected_index(Some(IndexPath::new(1)), window, cx);
            list.delegate_mut().confirm(false, window, cx);
        });
        assert_eq!(confirmed.lock().unwrap().as_deref(), Some("b.txt"));
    }
}
