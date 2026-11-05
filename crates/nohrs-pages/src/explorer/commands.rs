//! What the explorer's shortcuts run, and the "open this folder in another
//! program" commands of the header's menu.
//!
//! The key handler in `view.rs` asks `nohrs_core::shortcuts` which command a
//! key press means and hands it here; the keys themselves live in the
//! shortcut table and `[keybindings]`, never in this file.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{Context, Window};
use nohrs_core::shortcuts::Action;
use nohrs_core::telemetry::LogErr;
use nohrs_core::tf;

use super::ExplorerPane;
use super::types::StatusLevel;

/// A program the header's menu opens the folder in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenIn {
    CommandPrompt,
    PowerShell,
    WindowsTerminal,
    FileExplorer,
}

impl ExplorerPane {
    /// Runs the command a shortcut stands for. Returns false for the ones
    /// the page answers through gpui's actions (tabs, panes), so the key
    /// goes on to them.
    pub(crate) fn run_shortcut(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match action {
            Action::Copy => self.copy_selection(cx),
            Action::Cut => self.cut_selection(cx),
            Action::Paste => self.paste_into_cwd(window, cx),
            Action::Rename => {
                if let Some(index) = self.active_row() {
                    self.begin_rename(index, window, cx);
                }
            }
            Action::Refresh => self.reload(cx),
            Action::Trash => self.trash_selection(cx),
            Action::DeletePermanently => self.request_permanent_delete(window, cx),
            Action::NewFolder => self.create_new_folder(window, cx),
            Action::GoUp => self.go_up(window, cx),
            Action::SelectAll => {
                self.select_all();
                cx.notify();
            }
            Action::SearchSubfolders => self.set_quick_recursive(true, window, cx),
            Action::SearchEverything => {
                let on = !self.everything_on();
                self.set_everything(on, cx);
                self.focus_quick_input(None, window, cx);
            }
            Action::SearchIndex => self.toggle_search(window, cx),
            Action::FilterBar => self.toggle_filter_bar(cx),
            Action::ZoomIn => {
                self.zoom_to(super::zoom::step(self.zoom_target, super::zoom::STEP), cx);
                self.flash_zoom_menu(cx);
            }
            Action::ZoomOut => {
                self.zoom_to(super::zoom::step(self.zoom_target, -super::zoom::STEP), cx);
                self.flash_zoom_menu(cx);
            }
            Action::ZoomReset => {
                self.zoom_to(super::zoom::DEFAULT_GRID, cx);
                self.flash_zoom_menu(cx);
            }
            Action::OpenCommandPrompt => self.open_in(OpenIn::CommandPrompt, cx),
            Action::OpenPowerShell => self.open_in(OpenIn::PowerShell, cx),
            Action::OpenFileExplorer => self.open_in(OpenIn::FileExplorer, cx),
            Action::NewTab
            | Action::CloseTab
            | Action::SplitVertical
            | Action::SplitHorizontal
            | Action::FocusPane1
            | Action::FocusPane2
            | Action::NextPane
            | Action::PreviousPane
            | Action::ToggleSidebar
            | Action::ToggleTerminal => return false,
        }
        true
    }

    /// Shows the zoom menu for a moment after the zoom changed by wheel or
    /// key, so the slider says where it went, as File Pilot does. A menu
    /// opened by a click stays until a click elsewhere; one shown this way
    /// goes away by itself unless the mouse is on it.
    pub(crate) fn flash_zoom_menu(&mut self, cx: &mut Context<Self>) {
        if self.zoom_menu_open && self.zoom_menu_flash.is_none() {
            return;
        }
        self.zoom_menu_open = true;
        // Replacing the task drops the previous timer: the menu stays while
        // the wheel keeps turning.
        self.zoom_menu_flash = Some(cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
            cx.background_executor().timer(Duration::from_millis(1300)).await;
            this.update(cx, |pane, cx| {
                if pane.zoom_menu_hovered || pane.zoom_dragging {
                    // Taken over by the mouse: it closes like a clicked menu.
                    pane.zoom_menu_flash = None;
                    return;
                }
                pane.zoom_menu_open = false;
                pane.zoom_menu_flash = None;
                cx.notify();
            })
            .log_err();
        }));
        cx.notify();
    }

    /// The folder on screen as a place a program can start in: This PC has
    /// none, so programs other than Explorer start in the home folder.
    fn folder_for_programs(&self) -> PathBuf {
        let cwd = PathBuf::from(&self.cwd);
        if cwd.is_dir() {
            cwd
        } else {
            std::env::var_os("USERPROFILE").map_or_else(|| PathBuf::from("C:\\"), PathBuf::from)
        }
    }

    /// Opens the folder on screen in `program`, in a window of its own.
    pub(crate) fn open_in(&mut self, program: OpenIn, cx: &mut Context<Self>) {
        self.open_in_menu = false;
        let folder = self.folder_for_programs();
        let outcome = match program {
            OpenIn::FileExplorer => {
                // Explorer takes This PC by its shell name too.
                let target = if self.cwd.starts_with("::") {
                    std::ffi::OsString::from(&self.cwd)
                } else {
                    folder.as_os_str().to_owned()
                };
                std::process::Command::new("explorer.exe").arg(target).spawn().map(drop)
            }
            OpenIn::CommandPrompt => console(std::process::Command::new("cmd.exe").arg("/K"), &folder),
            OpenIn::PowerShell => {
                let mut command = std::process::Command::new(powershell());
                command.args(["-NoExit", "-NoLogo"]);
                console(&mut command, &folder)
            }
            OpenIn::WindowsTerminal => std::process::Command::new("wt.exe").arg("-d").arg(&folder).spawn().map(drop),
        };
        if let Err(error) = outcome {
            self.set_status(StatusLevel::Error, tf!("Não foi possível abrir o programa: {error}", error = error));
        }
        cx.notify();
    }
}

// Starts a console program in a console window of its own: Prime Files has
// none to lend it.
fn console(command: &mut std::process::Command, folder: &std::path::Path) -> std::io::Result<()> {
    command.current_dir(folder);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        command.creation_flags(CREATE_NEW_CONSOLE);
    }
    command.spawn().map(drop)
}

/// PowerShell 7 when installed, Windows PowerShell otherwise, as the
/// terminal panel picks.
fn powershell() -> PathBuf {
    let installed = std::env::var_os("ProgramFiles")
        .map(|folder| PathBuf::from(folder).join("PowerShell").join("7").join("pwsh.exe"))
        .filter(|path| path.is_file());
    installed.unwrap_or_else(|| PathBuf::from("powershell.exe"))
}

/// A `&'static str` for text built at run time, for the menus whose
/// entries hold static text. Each distinct text is kept once: a handful of
/// labels and key names, however often the menus open.
pub(crate) fn interned(text: String) -> &'static str {
    static SEEN: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(kept) = seen.iter().find(|kept| **kept == text) {
        return kept;
    }
    let kept: &'static str = Box::leak(text.into_boxed_str());
    seen.push(kept);
    kept
}

/// The keys of `action`, as a menu entry shows them; empty when it has none.
pub(crate) fn keys_of(action: Action) -> &'static str {
    nohrs_core::shortcuts::display(action).map_or("", interned)
}

/// Whether Windows Terminal is there to offer.
pub(crate) fn has_windows_terminal() -> bool {
    static FOUND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FOUND.get_or_init(|| {
        std::env::var_os("LOCALAPPDATA")
            .map(|folder| PathBuf::from(folder).join("Microsoft").join("WindowsApps").join("wt.exe"))
            .is_some_and(|path| path.exists())
    })
}
