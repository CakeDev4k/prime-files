use crate::explorer::ExplorerPane;
use gpui::prelude::*;
use gpui::*;
use gpui_component::text::TextView;
use gpui_component::{Icon, IconName};
use nohrs_ui::theme::theme;

/// Edge of the system thumbnail asked for when the pane has no richer preview.
const PREVIEW_THUMBNAIL_SIZE: u32 = 512;

// Calculate the maximum line width in characters for horizontal scroll sizing
/// Renders the preview pane for the selected file, showing a text editor,
/// image, status message, or an empty placeholder.
pub fn render(
    page: &mut ExplorerPane,
    window: &mut Window,
    cx: &mut Context<ExplorerPane>,
) -> impl IntoElement + use<> {
    let title = page
        .preview_path
        .as_ref()
        .map(|p| path_name(p))
        .unwrap_or_else(|| "Preview".to_string());

    let content = if let Some(editor) = &page.preview_editor {
        div().flex_1().child(editor.clone()).into_any_element()
    } else if let Some(markdown) = page.preview_markdown.clone() {
        // Keyed by path so the view's scroll and parse state belong to one
        // file and do not carry over to the next one opened.
        let id = SharedString::from(format!(
            "preview-markdown:{}",
            page.preview_path.as_deref().unwrap_or_default()
        ));
        div()
            .flex_1()
            .min_h(px(0.0))
            .min_w(px(0.0))
            .overflow_hidden()
            .px(px(16.0))
            .py(px(12.0))
            .text_sm()
            .text_color(rgb(theme::fg()))
            .child(
                TextView::markdown(id, markdown, window, cx)
                    .selectable(true)
                    .scrollable(true)
                    .size_full(),
            )
            .into_any_element()
    } else if let Some(image_path) = &page.preview_image_path {
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(theme::preview_backdrop()))
            // As with the thumbnail below: the image's natural width must not
            // become the pane's minimum.
            .min_w(px(0.0))
            .overflow_hidden()
            .child(
                // A `PathBuf`, not the string: GPUI reads a string source as a
                // URL or an embedded asset, so a plain path never loaded and the
                // pane stayed empty.
                img(std::path::PathBuf::from(image_path))
                    .min_w(px(0.0))
                    .h_full()
                    .w_full()
                    .object_fit(gpui::ObjectFit::Contain),
            )
            .into_any_element()
    } else if let Some(msg) = &page.preview_message {
        // No text and no image the pane can draw itself (a video, a program, an
        // archive): the system thumbnail still says what the file is, often a
        // frame of the video or the program's own icon.
        let thumbnail = page
            .preview_path
            .clone()
            .and_then(|path| page.thumbnails.get(&path, PREVIEW_THUMBNAIL_SIZE, cx));
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .p(px(16.0))
            // Without these the image's own 512px became the pane's minimum
            // width and pushed the divider over.
            .min_w(px(0.0))
            .overflow_hidden()
            .when_some(thumbnail, |this, image| {
                this.child(
                    img(ImageSource::Render(image))
                        .min_w(px(0.0))
                        .w_full()
                        .max_h(px(PREVIEW_THUMBNAIL_SIZE as f32))
                        .flex_shrink()
                        .object_fit(gpui::ObjectFit::Contain),
                )
            })
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(theme::muted()))
                    .child(msg.clone()),
            )
            .into_any_element()
    } else {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                Icon::new(IconName::File)
                    .size_8()
                    .text_color(rgb(theme::gray_300())),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(theme::muted()))
                    .child("No file selected"),
            )
            .into_any_element()
    };

    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(rgb(theme::bg()))
        .child(
            // Header. Matches the listing's column-header height so the two
            // line up across the divider between the panes.
            div()
                .h(px(super::listing::list::HEADER_HEIGHT))
                .flex()
                .items_center()
                .flex_shrink_0()
                .px(px(16.0))
                .border_b_1()
                .border_color(rgb(theme::border()))
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::fg()))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(title),
                ),
        )
        .child(content)
}

fn path_name(p: &str) -> String {
    std::path::Path::new(p)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string())
}

/// The text editor used to render file previews.
pub mod editor;
