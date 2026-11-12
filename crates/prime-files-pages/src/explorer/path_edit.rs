//! The path bar as text: a double click on the breadcrumb turns it into a field
//! holding the folder's full path, already selected, as Explorer's address bar
//! does — so Ctrl+C copies it, or a path typed over it is gone to on Enter.

use prime_files_core::tf;
use gpui::{AppContext, Context, Entity, EntityInputHandler, Subscription, Window};
use gpui_component::input::{InputEvent, InputState};
use prime_files_core::telemetry::LogErr as _;
use std::path::Path;

use super::ExplorerPane;
use super::types::StatusLevel;

/// An open path field.
pub(crate) struct PathEditState {
    /// The field showing the path.
    pub input: Entity<InputState>,
    // Drives Enter / Blur while the field is open; dropped with it.
    _subscription: Subscription,
}

impl ExplorerPane {
    /// Replaces the breadcrumb with a field holding the current path, selected.
    pub(crate) fn begin_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_edit.is_some() {
            return;
        }
        let path = self.cwd.clone();
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |state, cx| {
            // `set_value` leaves the caret at the end with nothing selected;
            // inserting through the IME entry point is the one public way to
            // seed the text with a selection (the rename field does the same).
            // The selection is counted in UTF-16, as the IME counts it.
            let length = path.encode_utf16().count();
            state.replace_and_mark_text_in_range(Some(0..0), &path, Some(0..length), window, cx);
            state.unmark_text(window, cx);
            state.focus(window, cx);
        });
        let subscription = cx.subscribe_in(&input, window, |this, _input, event: &InputEvent, window, cx| {
            match event {
                InputEvent::PressEnter { .. } => this.commit_path_edit(window, cx),
                // Clicking away puts the breadcrumb back, as Explorer does,
                // without going anywhere.
                InputEvent::Blur => this.cancel_path_edit(window, cx),
                _ => {}
            }
        });
        self.path_edit = Some(PathEditState { input, _subscription: subscription });
        cx.notify();
    }

    /// Closes the field without navigating.
    pub(crate) fn cancel_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.path_edit.take().is_some() {
            cx.focus_self(window);
            cx.notify();
        }
    }

    // Goes to the folder typed, or to the folder of a file typed. Anything
    // else leaves the field open so the typo can be fixed.
    pub(crate) fn commit_path_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = &self.path_edit else {
            return;
        };
        let typed = super::network::normalize_address(&state.input.read(cx).value());
        // A network address is gone to without asking the disk here: a
        // server that does not answer would freeze the window for as long,
        // and the listing reads it on a thread of its own (asking for a
        // login if the share wants one).
        if super::network::is_unc(&typed) {
            self.go_to_typed(typed, window, cx);
            return;
        }
        // Anything else is asked on another thread too: a mapped drive whose
        // server is gone (`Z:\`) held the window at every Enter until
        // 12/11/2026 (docs/PERFORMANCE.md, pass 19). The field stays open
        // meanwhile, so a typo found missing is still there to fix.
        let found = cx.background_spawn({
            let typed = typed.clone();
            async move {
                let path = Path::new(&typed);
                if path.is_dir() {
                    Some(typed.clone())
                } else if path.is_file() {
                    path.parent().map(|parent| parent.to_string_lossy().into_owned())
                } else {
                    None
                }
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let target = found.await;
            this.update_in(cx, |pane, window, cx| {
                // Typed over or closed while the disk answered: that newer
                // intent wins.
                let still_typed = pane.path_edit.as_ref().is_some_and(|state| {
                    super::network::normalize_address(&state.input.read(cx).value()) == typed
                });
                if !still_typed {
                    return;
                }
                match target {
                    Some(target) => pane.go_to_typed(target, window, cx),
                    None => {
                        pane.set_status(StatusLevel::Error, tf!("Pasta não encontrada: {path}", path = typed));
                        cx.notify();
                    }
                }
            })
            .log_err();
        })
        .detach();
    }

    fn go_to_typed(&mut self, target: String, window: &mut Window, cx: &mut Context<Self>) {
        self.path_edit = None;
        cx.focus_self(window);
        self.change_dir(target, window, cx);
        cx.notify();
    }
}
