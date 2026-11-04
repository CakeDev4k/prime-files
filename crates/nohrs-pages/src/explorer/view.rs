use crate::explorer::ExplorerPane;
use gpui::prelude::FluentBuilder;
use gpui::*;
use nohrs_ui::theme::theme;

/// Modal dialogs for file operations (permanent-delete confirm, paste conflict).
mod dialogs;
/// The filter bar: narrowing the listing by name pattern and kind.
mod filter_bar;
/// The explorer header with navigation controls and the path bar.
pub mod header;
/// The main file listing, in list or grid mode, with the search bar.
pub mod listing;
/// The file preview pane.
pub mod preview;
/// The explorer sidebar with quick-access locations.
pub mod sidebar;
/// The filter and subfolder-search field under the listing.
pub mod quick_bar;
/// The drawn context menus.
pub mod menu;
/// Sidebar, listing and preview side by side, with draggable dividers.
pub(crate) mod panels;

/// Renders the explorer page: header, sidebar, listing, and preview panes.
pub fn render(
    page: &mut ExplorerPane,
    window: &mut Window,
    cx: &mut Context<ExplorerPane>,
) -> impl IntoElement + use<> {
    page.ensure_loaded(cx);
    page.tick_zoom(window, cx);
    page.tick_scroll(window, cx);
    page.tick_marquee(window, cx);
    if std::mem::take(&mut page.preview_selection_pending)
        && let Some(path) = page.active_path.clone()
    {
        page.open_preview(path, window, cx);
    }
    page.update_editor_search(window, cx);
    if !page.focus_requested {
        page.focus_requested = true;
        cx.focus_self(window);
    }

    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(rgb(theme::bg()))
        .relative()
        .track_focus(&page.focus_handle)
        .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
            let key_lc = event.keystroke.key.to_lowercase();
            let modifiers = event.keystroke.modifiers;
            let with_modifier = modifiers.platform || modifiers.control;
            let is_f = key_lc == "f" || event.keystroke.key == "KeyF";
            // Escape closes an open menu before anything else hears it.
            if key_lc == "escape" && this.context_menu.is_some() {
                this.close_menu(cx);
                cx.stop_propagation();
                return;
            }
            // Escape while a browser waits on Nohrs hands its dialog back.
            if key_lc == "escape" && crate::file_dialog::picking().is_some() {
                crate::file_dialog::cancel();
                cx.stop_propagation();
                cx.notify();
                return;
            }
            // Escape abandons an open rename before anything else can claim it —
            // the field has focus, so this is the only Escape the user means.
            // The path field is a text field like any other: Escape closes
            // it, and every other key is its own (Ctrl+C copies the path, not
            // the selected files).
            if this.path_edit.is_some() {
                if key_lc == "escape" {
                    this.cancel_path_edit(window, cx);
                    cx.stop_propagation();
                }
                return;
            }
            if key_lc == "escape" && this.renaming.is_some() {
                this.cancel_rename(window, cx);
                cx.stop_propagation();
                return;
            }
            let close_with_escape = key_lc == "escape" && this.search_visible;
            if (is_f && with_modifier && modifiers.shift) || close_with_escape {
                this.toggle_search(window, cx);
                cx.stop_propagation();
                return;
            }
            if is_f && with_modifier {
                this.set_quick_recursive(true, window, cx);
                cx.stop_propagation();
                return;
            }
            let quick_bar_active = this.deep_search.is_some() || !this.name_filter.is_empty();
            if key_lc == "escape" && quick_bar_active {
                this.clear_quick_input(window, cx);
                cx.stop_propagation();
                return;
            }
            // A letter typed on the listing goes to the filter, as in File
            // Pilot; with a modifier it is a shortcut, and space is left to
            // the listing.
            if this.focus_handle.is_focused(window)
                && !with_modifier
                && !modifiers.alt
                && let Some(text) = event.keystroke.key_char.as_deref()
                && text.chars().count() == 1
                && text.chars().all(|character| !character.is_control() && !character.is_whitespace())
            {
                this.focus_quick_input(Some(text), window, cx);
                cx.stop_propagation();
                return;
            }
            // File operations (§1, canonical keys in §6). Each arm consumes the
            // event; anything unmatched falls through to the selection model below.
            let shift = modifiers.shift;
            match key_lc.as_str() {
                "delete" if shift => {
                    this.request_permanent_delete(window, cx);
                    cx.stop_propagation();
                    return;
                }
                "delete" => {
                    this.trash_selection(cx);
                    cx.stop_propagation();
                    return;
                }
                // macOS also trashes on Backspace (§6); on Linux only Delete does.
                "backspace" if cfg!(target_os = "macos") && !shift => {
                    this.trash_selection(cx);
                    cx.stop_propagation();
                    return;
                }
                "c" if with_modifier && !shift => {
                    this.copy_selection(cx);
                    cx.stop_propagation();
                    return;
                }
                "x" if with_modifier && !shift => {
                    this.cut_selection(cx);
                    cx.stop_propagation();
                    return;
                }
                "v" if with_modifier && !shift => {
                    this.paste_into_cwd(window, cx);
                    cx.stop_propagation();
                    return;
                }
                // Ctrl+Shift+L, as the filter toggle in spreadsheets and
                // VS Code's file filters; Ctrl+F already opens the search.
                "l" if with_modifier && shift => {
                    this.toggle_filter_bar(cx);
                    cx.stop_propagation();
                    return;
                }
                "n" if with_modifier && shift => {
                    this.create_new_folder(window, cx);
                    cx.stop_propagation();
                    return;
                }
                // F5 rereads the folder, as in Explorer. The right-click menu
                // already showed "Atualizar  F5" with nothing behind the key.
                "f5" if !with_modifier => {
                    this.reload(cx);
                    cx.stop_propagation();
                    return;
                }
                // Rename: F2 everywhere, plus Enter on macOS (§6).
                "f2" => {
                    if let Some(index) = this.active_row() {
                        this.begin_rename(index, window, cx);
                    }
                    cx.stop_propagation();
                    return;
                }
                // Enter opens the active entry on Windows, as in Explorer: a
                // folder is entered, a file opened (or sent to a waiting
                // browser dialog). macOS keeps Enter for rename, below.
                // Only with the listing itself focused: Enter in the search or
                // filter field bubbles up here too, and means something else.
                "enter"
                    if !cfg!(target_os = "macos")
                        && !with_modifier
                        && this.focus_handle.is_focused(window) =>
                {
                    if let Some(item) = this
                        .active_row()
                        .and_then(|index| this.filtered_entries.get(index).cloned())
                    {
                        this.activate_entry(item, window, cx);
                    }
                    cx.stop_propagation();
                    return;
                }
                "enter" if cfg!(target_os = "macos") && !with_modifier => {
                    if let Some(index) = this.active_row() {
                        this.begin_rename(index, window, cx);
                    }
                    cx.stop_propagation();
                    return;
                }
                // Zoom, as in browsers and Explorer. `+` because on most layouts
                // it is Shift+= and some report it under that name.
                "=" | "+" if with_modifier => {
                    this.zoom_to(super::zoom::step(this.zoom_target, super::zoom::STEP), cx);
                    cx.stop_propagation();
                    return;
                }
                "-" if with_modifier => {
                    this.zoom_to(super::zoom::step(this.zoom_target, -super::zoom::STEP), cx);
                    cx.stop_propagation();
                    return;
                }
                "0" if with_modifier => {
                    this.zoom_to(super::zoom::DEFAULT_GRID, cx);
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
            // Selection model (§5). Escape while searching is handled above, so
            // here it only clears the selection.
            match key_lc.as_str() {
                "a" if with_modifier => {
                    this.select_all();
                    cx.stop_propagation();
                    cx.notify();
                }
                // Search-close Escape is handled by the branch above (which
                // returns early), so here Escape only clears a selection — and
                // only consumes the event when there was one to clear, leaving
                // an empty-selection Escape free to bubble.
                "escape" if !this.selection.is_empty() => {
                    this.clear_selection();
                    cx.stop_propagation();
                    cx.notify();
                }
                // In the grid Up/Down cross a row and Left/Right step along
                // it; in the list a row is a single entry.
                "up" => {
                    this.move_active_following(-this.row_stride(), modifiers.shift, window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
                "down" => {
                    this.move_active_following(this.row_stride(), modifiers.shift, window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
                // In the List view a column is a run of entries; sideways
                // crosses a whole column.
                "left" if this.view_mode == super::types::ViewMode::Compact => {
                    this.move_active_following(-(this.compact_rows.max(1) as isize), modifiers.shift, window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
                "right" if this.view_mode == super::types::ViewMode::Compact => {
                    this.move_active_following(this.compact_rows.max(1) as isize, modifiers.shift, window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
                // Tree: Right opens the folder, Left closes it, as in
                // Explorer's navigation pane.
                "right" | "left" if this.view_mode == super::types::ViewMode::Tree => {
                    let open = key_lc == "right";
                    if let Some(folder) = this
                        .active_row()
                        .and_then(|row| this.filtered_entries.get(row))
                        .filter(|entry| entry.kind == "dir")
                        .map(|entry| entry.path.clone())
                        && this.tree_open(&folder) != open
                    {
                        this.toggle_tree_folder(folder, cx);
                    }
                    cx.stop_propagation();
                    cx.notify();
                }
                "left" if this.view_mode == super::types::ViewMode::Grid => {
                    this.move_active_following(-1, modifiers.shift, window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
                "right" if this.view_mode == super::types::ViewMode::Grid => {
                    this.move_active_following(1, modifiers.shift, window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
                _ => {}
            }
        }))
        .on_mouse_move(
            cx.listener(|this, event: &gpui::MouseMoveEvent, _window, cx| {
                if event.pressed_button == Some(gpui::MouseButton::Left)
                    && this.update_divider_drag(event.position.x)
                {
                    cx.notify();
                    return;
                }
                if this.resizing_column.is_some() {
                    this.update_column_resize(event.position);
                    cx.notify();
                }
            }),
        )
        .on_mouse_up(
            gpui::MouseButton::Left,
            cx.listener(|this, _event, _window, cx| {
                if this.divider_drag.take().is_some() {
                    cx.notify();
                }
                if this.resizing_column.is_some() {
                    this.stop_column_resize();
                    cx.notify();
                }
            }),
        )
        .child(header::render(page, window, cx))
        .when(page.filter_bar_visible, |this| {
            this.child(filter_bar::render(page, window, cx))
        })
        .child({
            let sidebar = if page.sidebar_visible {
                sidebar::render(page, window, cx).into_any_element()
            } else {
                div().into_any_element()
            };
            let listing = listing::render(page, window, cx);
            let preview = if page.preview_visible {
                preview::render(page, window, cx).into_any_element()
            } else {
                div().into_any_element()
            };
            panels::render(page, sidebar, listing, preview, cx)
        })
        .child(quick_bar::render(page, window, cx))
        .children(menu::render(page, window, cx))
}

/// `text` with every occurrence of `query` in the accent colour, as File
/// Pilot marks what a filter or search matched.
pub fn highlighted_name(text: &str, query: &str) -> gpui::StyledText {
    let accent = gpui::HighlightStyle {
        color: Some(rgb(theme::accent()).into()),
        ..Default::default()
    };
    let ranges = find_query_highlights(text, query)
        .into_iter()
        .map(|(range, _)| (range, accent))
        .collect::<Vec<_>>();
    gpui::StyledText::new(text.to_string()).with_highlights(ranges)
}

/// The "› folder" after a search hit's name, muted, as File Pilot shows it.
pub fn hit_folder_label(folder: &str) -> gpui::Div {
    div()
        .flex_shrink()
        .min_w(px(0.0))
        .pl(px(6.0))
        .text_xs()
        .text_color(rgb(theme::muted()))
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .child(format!("›  {folder}"))
}

/// Returns highlight ranges for every case-insensitive occurrence of `query`
/// within `text`, for emphasizing search matches.
pub fn find_query_highlights(
    text: &str,
    query: &str,
) -> Vec<(std::ops::Range<usize>, gpui::HighlightStyle)> {
    let mut highlights = Vec::new();
    if query.is_empty() {
        return highlights;
    }

    let query_lower: Vec<char> = query.to_lowercase().chars().collect();
    let text_chars: Vec<(usize, char)> = text.char_indices().collect();

    let mut i = 0;
    while i < text_chars.len() {
        let mut match_found = true;
        let mut q_idx = 0;

        let mut current_t_offset = 0;

        while q_idx < query_lower.len() {
            if i + current_t_offset >= text_chars.len() {
                match_found = false;
                break;
            }

            let (_, t_char) = text_chars[i + current_t_offset];
            let t_lower = t_char.to_lowercase();

            for tc in t_lower {
                if q_idx >= query_lower.len() || query_lower[q_idx] != tc {
                    match_found = false;
                    break;
                }
                q_idx += 1;
            }

            if !match_found {
                break;
            }
            current_t_offset += 1;
        }

        if match_found && q_idx == query_lower.len() {
            let start_byte = text_chars[i].0;
            let end_byte = if i + current_t_offset < text_chars.len() {
                text_chars[i + current_t_offset].0
            } else {
                text.len()
            };

            highlights.push((
                start_byte..end_byte,
                gpui::HighlightStyle {
                    background_color: Some(gpui::Hsla::from(gpui::Rgba {
                        r: 1.0,
                        g: 0.9,
                        b: 0.0,
                        a: 0.5,
                    })),
                    color: Some(gpui::Hsla::from(gpui::Rgba {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    })),
                    ..Default::default()
                },
            ));

            i += current_t_offset;
        } else {
            i += 1;
        }
    }

    highlights.retain(|(range, _)| {
        let start_ok = text.is_char_boundary(range.start);
        let end_ok = text.is_char_boundary(range.end);
        if !start_ok || !end_ok {
            if std::env::var("NOHR_DEBUG").is_ok() {
                tracing::error!("[CRITICAL] find_query_highlights: Removing invalid highlight: {:?} (start_ok={}, end_ok={}) in text len {}", range, start_ok, end_ok, text.len());
            }
            return false;
        }
        true
    });

    highlights
}
