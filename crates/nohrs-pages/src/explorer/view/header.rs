use nohrs_core::{k, t};
use nohrs_core::i18n;
use super::super::types::ViewMode;
use crate::explorer::ExplorerPane;
use gpui::prelude::*;
use gpui::*;
use gpui_component::breadcrumb::{Breadcrumb, BreadcrumbItem};
use gpui_component::Sizable;
use gpui_component::input::Input;
use gpui_component::list::ListItem;
use gpui_component::{Icon, IconName};
use nohrs_ui::theme::theme;

/// Renders the explorer header with navigation buttons, the breadcrumb path
/// bar, and view-mode controls.
pub fn render(
    page: &mut ExplorerPane,
    _window: &mut Window,
    cx: &mut Context<ExplorerPane>,
) -> impl IntoElement + use<> {
    // This PC is an address, not a path: one crumb with its name. Under a
    // drive, a crumb for it comes first, as in Explorer, leading back to it.
    let location = page.location_name();
    let parts = if location.is_some() { Vec::new() } else { path_parts(&page.cwd) };
    let under_this_pc = location.is_none() && starts_with_drive(&page.cwd);
    let this_pc_name = page
        .sidebar
        .places
        .as_ref()
        .and_then(|places| places.this_pc.as_ref())
        .map(|place| place.name.clone())
        .unwrap_or_else(|| i18n::tr(crate::explorer::places::THIS_PC_NAME).to_string());

    let (display_parts, is_truncated) = if parts.len() > 5 {
        (parts[(parts.len() - 5)..].to_vec(), true)
    } else {
        (parts.clone(), false)
    };

    let mut bc = Breadcrumb::new();
    if let Some(name) = location {
        bc = bc.child(BreadcrumbItem::new(name));
    } else if under_this_pc {
        bc = bc.child(BreadcrumbItem::new(this_pc_name).on_click(cx.listener(|this, _, window, cx| {
            this.change_dir(crate::explorer::places::THIS_PC.to_string(), window, cx)
        })));
    }

    if is_truncated {
        bc = bc.child(BreadcrumbItem::new("…").on_click(cx.listener(move |_this, _, _, _| {})));
    }

    let start_idx = if is_truncated { parts.len() - 5 } else { 0 };

    for (display_i, p) in display_parts.iter().enumerate() {
        let actual_i = start_idx + display_i;
        let text = if p.is_empty() {
            String::from("/")
        } else {
            p.clone()
        };

        // `PathBuf::push` knows the root component already ends in a separator;
        // joining the strings by hand produced "//tmp" for every crumb under "/".
        let mut prefix = std::path::PathBuf::new();
        for (j, part) in parts.iter().enumerate() {
            prefix.push(part);
            if j >= actual_i {
                break;
            }
        }
        let mut path_here = prefix.to_string_lossy().to_string();
        if path_here.is_empty() {
            path_here = page.cwd.clone();
        }

        bc = bc.child(BreadcrumbItem::new(text).on_click(
            cx.listener(move |this, _, window, cx| this.change_dir(path_here.clone(), window, cx)),
        ));
    }

    let can_go_back = page.history_index > 0;
    let can_go_forward = page.history_index + 1 < page.history.len();
    let can_go_up = crate::explorer::navigation::parent_location(&page.cwd).is_some();

    // Store search_visible for use in search toggle style
    let preview_visible = page.preview_visible;
    let entry_count = page.filtered_entries.len();

    let framed = page.framed.header;
    div()
        .when(!framed, |this| this.bg(rgb(theme::bg())).border_b_1())
        .border_color(rgb(theme::border()))
        .flex()
        .items_center()
        .text_color(rgb(theme::fg()))
        .px(px(16.0))
        .py(px(10.0))
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .flex_shrink_0()
                .child(
                    ListItem::new("nav-back")
                        .px(px(8.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .when(!can_go_back, |this| this.opacity(0.35))
                        .when(can_go_back, |this| {
                            this.on_click(
                                cx.listener(|view, _, window, cx| view.go_back(window, cx)),
                            )
                        })
                        .child(
                            Icon::new(IconName::ArrowLeft)
                                .size_4()
                                .text_color(rgb(theme::gray_600())),
                        ),
                )
                .child(
                    ListItem::new("nav-forward")
                        .px(px(8.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .when(!can_go_forward, |this| this.opacity(0.35))
                        .when(can_go_forward, |this| {
                            this.on_click(
                                cx.listener(|view, _, window, cx| view.go_forward(window, cx)),
                            )
                        })
                        .child(
                            Icon::new(IconName::ArrowRight)
                                .size_4()
                                .text_color(rgb(theme::gray_600())),
                        ),
                )
                // Up and refresh beside back and forward, in Explorer's order.
                .child(
                    ListItem::new("nav-up")
                        .px(px(8.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .when(!can_go_up, |this| this.opacity(0.35))
                        .when(can_go_up, |this| {
                            this.on_click(cx.listener(|view, _, window, cx| view.go_up(window, cx)))
                        })
                        .child(
                            Icon::new(Icon::empty())
                                .path("icons/arrow-up.svg")
                                .size_4()
                                .text_color(rgb(theme::gray_600())),
                        ),
                )
                .child(
                    ListItem::new("nav-refresh")
                        .px(px(8.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .on_click(cx.listener(|view, _, _, cx| view.reload(cx)))
                        .child(
                            Icon::new(Icon::empty())
                                .path("icons/refresh-cw.svg")
                                .size_4()
                                .text_color(rgb(theme::gray_600())),
                        ),
                )
                .child(
                    div()
                        .w(px(1.0))
                        .h(px(20.0))
                        .bg(rgb(theme::border()))
                        .mx(px(6.0)),
                ),
        )
        .child(match &page.path_edit {
            Some(state) => div()
                .flex_1()
                .min_w(px(0.0))
                .child(Input::new(&state.input).small().text_sm())
                .into_any_element(),
            // A double click anywhere on the bar, crumbs included, turns it
            // into the full path; a single click on a crumb still navigates.
            None => div()
                .id("path-bar")
                .flex_1()
                .h(px(28.0))
                .flex()
                .items_center()
                .overflow_hidden()
                .min_w(px(0.0))
                .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                    if event.click_count() >= 2 {
                        this.begin_path_edit(window, cx);
                    }
                }))
                .child(bc)
                .into_any_element(),
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .flex_shrink_0()
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(theme::fg_secondary()))
                        .whitespace_nowrap()
                        .mr(px(4.0))
                        .child(i18n::plural(entry_count, k!("{n} item"), k!("{n} itens"))),
                )
                .child(render_view_mode_toggle(page, cx))
                .child(
                    // The preview switch sits up here, beside the view modes,
                    // where it is seen: the preview is one of the explorer's
                    // most used parts. The filter menu went to the bottom bar.
                    ListItem::new("preview-toggle")
                        .px(px(8.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.preview_visible = !view.preview_visible;
                            cx.notify();
                        }))
                        .child(
                            Icon::new(Icon::empty())
                                .path("icons/panel-right.svg")
                                .size_4()
                                .text_color(if preview_visible {
                                    rgb(theme::accent())
                                } else {
                                    rgb(theme::gray_600())
                                }),
                        ),
                ),
        )
}

fn render_view_mode_toggle(
    page: &mut ExplorerPane,
    cx: &mut Context<ExplorerPane>,
) -> impl IntoElement + use<> {
    div()
        .flex()
        .items_center()
        .gap_1()
        .p(px(2.0))
        .rounded(px(8.0))
        .bg(rgb(theme::bg_secondary()))
        .border_1()
        .border_color(rgb(theme::border()))
        // `list.svg` ships with this crate but has no `IconName` variant, so it
        // is loaded by asset path.
        .child(view_mode_button(
            page,
            ViewMode::List,
            "view-mode-list",
            Icon::new(Icon::empty()).path(SharedString::from("icons/list.svg")),
            t!("Lista"),
            cx,
        ))
        .child(view_mode_button(
            page,
            ViewMode::Grid,
            "view-mode-grid",
            Icon::new(IconName::LayoutDashboard),
            t!("Grade"),
            cx,
        ))
}

fn view_mode_button(
    page: &mut ExplorerPane,
    mode: ViewMode,
    id: &'static str,
    icon: Icon,
    label: &'static str,
    cx: &mut Context<ExplorerPane>,
) -> impl IntoElement + use<> {
    // Tree and List are views of the list side, so its button stays lit.
    let is_active = (page.view_mode == ViewMode::Grid) == (mode == ViewMode::Grid);
    // The active segment lifts onto white so its accent icon reads against the
    // group's tinted track, rather than accent-on-gray.
    ListItem::new(id)
        .px(px(8.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .when(is_active, |this| this.bg(rgb(theme::bg())).shadow_sm())
        .on_click(cx.listener(move |this, _, _, cx| this.set_view_mode(mode, cx)))
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .child(icon.size_4().text_color(if is_active {
                    rgb(theme::accent())
                } else {
                    rgb(theme::gray_500())
                }))
                .child(
                    div()
                        .text_xs()
                        .when(is_active, |this| this.font_weight(gpui::FontWeight::MEDIUM))
                        .text_color(if is_active {
                            rgb(theme::fg())
                        } else {
                            rgb(theme::fg_secondary())
                        })
                        .child(label),
                ),
        )
}

// `C:\...`: a path on a lettered drive, which Explorer shows under This PC.
fn starts_with_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn path_parts(path: &str) -> Vec<String> {
    use std::path::Component;
    let mut parts: Vec<String> = Vec::new();
    let mut after_prefix = false;
    for c in std::path::Path::new(path).components() {
        match (c, parts.last_mut()) {
            // Windows splits `C:\` (and `\\server\share\`) into a prefix and a
            // `\` root. Shown apart they are two crumbs where the user sees one
            // drive, and the first navigates to `C:`, which Windows reads as the
            // current directory on that drive rather than its root.
            (Component::RootDir, Some(prefix)) if after_prefix => {
                prefix.push(std::path::MAIN_SEPARATOR);
            }
            _ => parts.push(c.as_os_str().to_string_lossy().to_string()),
        }
        after_prefix = matches!(c, Component::Prefix(_));
    }
    if parts.is_empty() {
        parts.push(path.to_string());
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::path_parts;

    #[cfg(windows)]
    #[test]
    fn drive_and_root_are_one_crumb() {
        assert_eq!(path_parts(r"C:\Users\ada"), ["C:\\", "Users", "ada"]);
        assert_eq!(path_parts(r"C:\"), ["C:\\"]);
        assert_eq!(
            path_parts(r"\\server\share\docs"),
            [r"\\server\share\", "docs"]
        );
    }

    #[cfg(windows)]
    #[test]
    fn up_goes_to_the_parent_and_from_a_drive_to_this_pc() {
        use crate::explorer::navigation::parent_location;
        use crate::explorer::places::THIS_PC;
        assert_eq!(parent_location(r"C:\Users\ada").as_deref(), Some(r"C:\Users"));
        assert_eq!(parent_location(r"C:\Users").as_deref(), Some(r"C:\"));
        assert_eq!(parent_location(r"C:\").as_deref(), Some(THIS_PC));
        assert_eq!(parent_location(r"d:").as_deref(), Some(THIS_PC));
        assert_eq!(parent_location(THIS_PC), None);
    }

    #[cfg(unix)]
    #[test]
    fn root_stays_its_own_crumb() {
        assert_eq!(path_parts("/tmp/ada"), ["/", "tmp", "ada"]);
    }
}
