//! Dragging files: out of the listing to anywhere Windows accepts files
//! (Explorer, the desktop, Premiere, a browser upload), and into the listing
//! or onto one of its folders.
//!
//! One mechanism serves both directions. A drag out is a real Windows (OLE)
//! drag, so dropping it back on Prime Files arrives through the same door as a drag
//! from Explorer: gpui's drop target, as `ExternalPaths`. The drop side then
//! only has to handle that.

use prime_files_core::{t, tf};
use gpui::{Context, ExternalPaths, InteractiveElement, MouseMoveEvent, Pixels, Point, Styled, Window, rgb};
use prime_files_ui::theme::theme;
use std::path::{Path, PathBuf};

use super::ExplorerPane;
use super::file_ops::ClipMode;
use super::types::StatusLevel;

/// How far the pointer travels with the button held before a press becomes a
/// drag, as Windows' default drag rectangle (`SM_CXDRAG`, 4 px each way)
/// rounded up for high-DPI screens.
const DRAG_THRESHOLD: f32 = 6.0;

/// Where a drag out started, for what has to be read again once it ends.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DragFrom {
    Listing,
    Preview,
}

/// A left press on an entry that may turn into a drag.
pub(crate) struct DragCandidate {
    ix: usize,
    origin: Point<Pixels>,
    // The press landed on an entry of a multiple selection without modifiers:
    // the selection is narrowed to it on release, unless the press became a
    // drag of the whole selection, as Explorer does.
    narrow_on_release: bool,
}

/// A left press on an item listed in the preview — a file inside the folder the
/// preview shows — that may turn into a drag. Kept apart from
/// [`DragCandidate`], which addresses the listing by row: the preview's items
/// are not rows of the listing, and there is no selection to drag with them.
pub(crate) struct PreviewDragCandidate {
    source: PreviewDragSource,
    origin: Point<Pixels>,
}

enum PreviewDragSource {
    Disk(PathBuf),
    Archive(PathBuf, String, bool),
}

impl PreviewDragCandidate {
    /// What the drag would carry.
    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        match &self.source {
            PreviewDragSource::Disk(path) => path,
            PreviewDragSource::Archive(path, _, _) => path,
        }
    }
}

impl ExplorerPane {
    /// Records a left press on `path`, listed in the preview. A release
    /// without moving stays the click it was: the item opens in the preview.
    pub(crate) fn press_preview_item(&mut self, path: &str, position: Point<Pixels>) {
        self.preview_drag = Some(PreviewDragCandidate {
            source: PreviewDragSource::Disk(PathBuf::from(path)),
            origin: position,
        });
    }

    /// A press on the archive's own strip in the preview: dragged, the whole
    /// archive comes out as a folder named after it.
    pub(crate) fn press_preview_whole_archive(&mut self, archive: PathBuf, position: Point<Pixels>) {
        self.press_preview_archive_item(archive, String::new(), true, position);
    }

    pub(crate) fn press_preview_archive_item(
        &mut self,
        archive: PathBuf,
        member: String,
        is_dir: bool,
        position: Point<Pixels>,
    ) {
        self.preview_drag = Some(PreviewDragCandidate {
            source: PreviewDragSource::Archive(archive, member, is_dir),
            origin: position,
        });
    }

    /// Ends a press on a preview item that did not become a drag.
    pub(crate) fn release_preview_item(&mut self) {
        self.preview_drag = None;
    }

    /// Starts dragging the pressed preview item out once the pointer has
    /// moved far enough, the same Windows drag the listing starts: dropped on
    /// Premiere it imports, on Explorer it copies or moves, on Prime Files it lands
    /// like any drop.
    pub(crate) fn preview_drag_moved(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(candidate) = self.preview_drag_ready(event) else {
            return;
        };
        match candidate.source {
            PreviewDragSource::Disk(path) => {
                // A press on one of the items picked with Ctrl or Shift takes
                // them all, as a press on a selected row of the listing does.
                let picked = self
                    .preview_listing
                    .as_ref()
                    .map(|listing| self.preview_listing_selection.paths(listing))
                    .unwrap_or_default();
                let paths = if picked.len() > 1 && picked.contains(&path) { picked } else { vec![path] };
                self.start_drag_out(paths, DragFrom::Preview, window, cx)
            }
            PreviewDragSource::Archive(archive, member, is_dir) => {
                self.start_archive_drag_out(archive, member, is_dir, window, cx)
            }
        }
    }

    // The pressed preview item, once the press has moved past the threshold
    // with the button held; a press whose button came up is forgotten.
    pub(crate) fn preview_drag_ready(&mut self, event: &MouseMoveEvent) -> Option<PreviewDragCandidate> {
        let candidate = self.preview_drag.as_ref()?;
        if event.pressed_button != Some(gpui::MouseButton::Left) {
            self.preview_drag = None;
            return None;
        }
        let moved = event.position - candidate.origin;
        if f32::from(moved.x).abs() < DRAG_THRESHOLD && f32::from(moved.y).abs() < DRAG_THRESHOLD {
            return None;
        }
        self.preview_drag.take()
    }

    /// Records a left press on row `ix`. Returns whether the selection should
    /// be left as it is for now (the entry is part of a multiple selection).
    pub(crate) fn press_entry(&mut self, ix: usize, position: Point<Pixels>, plain: bool) -> bool {
        self.end_hover_audio();
        let narrow_on_release = plain && self.is_selected(ix) && self.selection.len() > 1;
        self.drag_candidate = Some(DragCandidate {
            ix,
            origin: position,
            narrow_on_release,
        });
        narrow_on_release
    }

    /// Ends a press that did not become a drag.
    pub(crate) fn release_entry(&mut self, cx: &mut Context<Self>) {
        if let Some(candidate) = self.drag_candidate.take()
            && candidate.narrow_on_release
        {
            self.select_single(candidate.ix);
            cx.notify();
        }
    }

    /// Starts a drag once a press has moved far enough.
    pub(crate) fn drag_moved(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(candidate) = &self.drag_candidate else {
            return;
        };
        if event.pressed_button != Some(gpui::MouseButton::Left) {
            self.drag_candidate = None;
            return;
        }
        let moved = event.position - candidate.origin;
        if f32::from(moved.x).abs() < DRAG_THRESHOLD && f32::from(moved.y).abs() < DRAG_THRESHOLD {
            return;
        }
        let Some(candidate) = self.drag_candidate.take() else {
            return;
        };
        if self.renaming.is_some() {
            return;
        }
        let Some(entry) = self.filtered_entries.get(candidate.ix) else {
            return;
        };
        // The selection if the press was on part of it, else just that entry.
        let paths: Vec<PathBuf> = if self.selection.contains(&entry.path) {
            self.selected_paths().into_iter().map(PathBuf::from).collect()
        } else {
            vec![PathBuf::from(&entry.path)]
        };
        self.start_drag_out(paths, DragFrom::Listing, window, cx);
    }

    fn start_drag_out(&mut self, paths: Vec<PathBuf>, from: DragFrom, window: &mut Window, cx: &mut Context<Self>) {
        // A drag of the selected item is no click on it.
        self.slow_rename = None;
        let Some(hwnd) = crate::ipc::window_hwnd(window) else {
            return;
        };
        // From a task rather than from this handler: the drag runs its own
        // message loop until the drop, during which gpui keeps painting and
        // receives the drop itself when it lands on this window. Here the app
        // is borrowed for the handler; in a task, between polls, it is not.
        cx.spawn_in(window, async move |this, cx| {
            let outcome = platform::drag(hwnd, &paths);
            this.update(cx, |pane, cx| {
                if let Err(error) = outcome {
                    pane.set_status(StatusLevel::Error, tf!("Não foi possível arrastar: {error}", error = error));
                }
                // A move to another program takes the files out of this folder.
                pane.reload(cx);
                if from == DragFrom::Preview {
                    // And out of the folder the preview lists.
                    pane.refresh_preview_listing(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_archive_drag_out(
        &mut self,
        archive: PathBuf,
        member: String,
        is_dir: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.slow_rename = None;
        let Some(hwnd) = crate::ipc::window_hwnd(window) else {
            return;
        };
        // The path the drag carries exists now, empty; the drag starts on it
        // while the button is still down (`preview_listing::DragStage`).
        // No member: the whole archive, as a folder named after it.
        let staged = if member.is_empty() {
            super::preview_listing::stage_whole_archive(&archive)
        } else {
            super::preview_listing::stage_for_drag(&member, is_dir)
        };
        let stage = match staged {
            Ok(stage) => stage,
            Err(error) => {
                self.set_status(StatusLevel::Error, tf!("Não foi possível arrastar: {error}", error = error));
                cx.notify();
                return;
            }
        };
        let target = stage.target.clone();
        // The extraction runs while the pointer travels, so a drop seldom
        // waits for all of it. A thread of its own rather than the background
        // executor: one large entry would hold one of its few workers for as
        // long as it takes, and the drop blocks on it either way.
        let extraction = std::sync::Arc::new(Extraction::default());
        let writer = extraction.clone();
        let entry = (archive.clone(), member.clone());
        self.archive_extracting = Some(entry.clone());
        cx.notify();
        std::thread::spawn(move || {
            writer.finish(super::preview_listing::extract_staged(&stage, &archive, &member, is_dir));
        });
        // The loading mark comes off when the extraction ends, not when the
        // drag does: an entry that is whole while the button is still down is
        // ready to drop, and the mark going away is what says so. gpui keeps
        // running tasks inside the drag's message loop, so this one does too.
        let watched = extraction.clone();
        cx.spawn_in(window, async move |this, cx| {
            while watched.result().is_none() {
                cx.background_executor().timer(std::time::Duration::from_millis(60)).await;
            }
            this.update(cx, |pane, cx| {
                if pane.archive_extracting.as_ref() == Some(&entry) {
                    pane.archive_extracting = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            let dragged = platform::drag_pending(hwnd, &[target], &extraction).map_err(|error| error.to_string());
            // A drop the extraction failed under was cancelled; its reason is
            // the one to show. A drag cancelled with Esc leaves the extraction
            // to end on its own, in a folder nothing points at.
            let outcome = match (dragged, extraction.result()) {
                (_, Some(Err(error))) | (Err(error), _) => Err(error),
                _ => Ok(()),
            };
            this.update(cx, |pane, cx| {
                if let Err(error) = outcome {
                    pane.set_status(StatusLevel::Error, tf!("Não foi possível arrastar: {error}", error = error));
                }
                pane.reload(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Copies or moves files dropped on the window into `dest`. Explorer's
    /// rules: Shift moves, Ctrl copies, and otherwise a drop on the same drive
    /// moves and one from another drive copies.
    pub(crate) fn drop_paths(&mut self, paths: &[PathBuf], dest: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // Onto a phone: the Shell copies, with its own progress window, as a
        // drop on the phone in the Explorer does.
        let address = dest.to_string_lossy().into_owned();
        if super::device::is_device(&address) {
            let sources: Vec<String> = paths.iter().map(|path| path.to_string_lossy().into_owned()).collect();
            let owner = crate::ipc::window_hwnd(window);
            self.run_device_op(move |owner| super::device::transfer(sources, address, false, owner), owner, cx);
            return;
        }
        // Files another program put in the temp folder for this drag (WinRAR,
        // 7-Zip) are its own, and it deletes them once the drag returns,
        // before the copy in the background reaches them: "1 of 1 failed"
        // (issue #64). They are held here first, while the program waits.
        // Not in tests, whose files all live in the temp folder.
        #[cfg(not(test))]
        let staged = hold_foreign_temp_files(paths);
        #[cfg(test)]
        let staged: Option<Vec<PathBuf>> = None;
        let paths = staged.as_deref().unwrap_or(paths);
        let DropPlan { mode, sources, into_itself } = drop_plan(paths, &dest, window.modifiers());
        if into_itself {
            self.set_status(StatusLevel::Warning, t!("Uma pasta não pode ir para dentro dela mesma."));
            cx.notify();
        }
        if sources.is_empty() {
            return;
        }
        if self.prepare_transfer(mode, sources, dest, false) {
            self.open_conflict_dialog(window, cx);
        } else {
            self.execute_paste_plan(cx);
        }
    }
}

/// Where dropped files from another program's temp folder are held.
const HELD_FOLDER: &str = "PrimeFiles-Dropped";

/// Held drops older than this are gone: the transfer that took them is over.
const HELD_KEPT: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// When some of `paths` lie in the temp folder and not in one of Prime
/// Files' own, holds them all in a folder of Prime Files' own: hard links,
/// which outlive the program deleting its copy and take no time or room
/// (the temp folder is one volume), or a copy where a link is refused. The
/// held files are the same volume as the temp folder, so the drop moves
/// them on that drive like any same-drive drop, and a copy elsewhere.
/// `None` leaves the drop as it was, and so does a hold that fails.
fn hold_foreign_temp_files(paths: &[PathBuf]) -> Option<Vec<PathBuf>> {
    let temp = std::env::temp_dir();
    let held_root = temp.join(HELD_FOLDER);
    let foreign = |path: &Path| {
        starts_with_ignoring_case(path, &temp)
            && !starts_with_ignoring_case(path, &held_root)
            && !starts_with_ignoring_case(path, &temp.join("PrimeFiles-ArchiveDrag"))
    };
    if !paths.iter().any(|path| foreign(path)) {
        return None;
    }
    sweep_held(&held_root);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or_default();
    let folder = held_root.join(format!("{stamp:x}"));
    let mut held = Vec::with_capacity(paths.len());
    for path in paths {
        if !foreign(path) {
            held.push(path.clone());
            continue;
        }
        let Some(name) = path.file_name() else {
            held.push(path.clone());
            continue;
        };
        let target = folder.join(name);
        if let Err(error) = std::fs::create_dir_all(&folder).and_then(|()| hold(path, &target)) {
            tracing::warn!("could not hold the dropped {}: {error}", path.display());
            return None;
        }
        held.push(target);
    }
    Some(held)
}

// Links `source` (a file, or a folder walked whole) at `target`, copying what
// cannot be linked.
fn hold(source: &Path, target: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(source)?.is_dir() {
        std::fs::create_dir_all(target)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            hold(&entry.path(), &target.join(entry.file_name()))?;
        }
        return Ok(());
    }
    match std::fs::hard_link(source, target) {
        Ok(()) => Ok(()),
        Err(_) => std::fs::copy(source, target).map(|_| ()),
    }
}

// Removes held drops from earlier transfers.
fn sweep_held(held_root: &Path) {
    let Ok(entries) = std::fs::read_dir(held_root) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > HELD_KEPT);
        if old && let Err(error) = std::fs::remove_dir_all(entry.path()) {
            tracing::debug!("could not clear the held drop {}: {error}", entry.path().display());
        }
    }
}

fn starts_with_ignoring_case(path: &Path, prefix: &Path) -> bool {
    let path = path.to_string_lossy().to_lowercase();
    let prefix = prefix.to_string_lossy().to_lowercase();
    let prefix = prefix.trim_end_matches(['\\', '/']);
    path.strip_prefix(prefix).is_some_and(|rest| rest.is_empty() || rest.starts_with(['\\', '/']))
}

/// What a drop of `paths` on `dest` does, by Explorer's rules.
struct DropPlan {
    mode: ClipMode,
    /// What actually goes: not a folder into itself, not a move to where
    /// the file already is.
    sources: Vec<PathBuf>,
    /// Some folder was dropped into itself or one of its subfolders.
    into_itself: bool,
}

// One rule for the cursor while the drag is over a folder and for the drop:
// Shift moves, Ctrl copies, otherwise the same drive moves and another
// drive copies.
fn drop_plan(paths: &[PathBuf], dest: &Path, modifiers: gpui::Modifiers) -> DropPlan {
    let mode = if modifiers.shift {
        ClipMode::Cut
    } else if modifiers.control {
        ClipMode::Copy
    } else if paths.iter().all(|path| same_volume(path, dest)) {
        ClipMode::Cut
    } else {
        ClipMode::Copy
    };
    let mut into_itself = false;
    let sources = paths
        .iter()
        .filter(|source| {
            let nested = dest.starts_with(source);
            into_itself |= nested;
            let in_place = mode == ClipMode::Cut && source.parent() == Some(dest);
            !nested && !in_place
        })
        .cloned()
        .collect();
    DropPlan { mode, sources, into_itself }
}

// What the cursor says over `dest`: "Move" or "Copy" as the drop will do,
// the no-entry sign when nothing would go (a folder into itself, files
// moved to the folder they are in).
fn drop_effect(paths: &[PathBuf], dest: &Path, modifiers: gpui::Modifiers) -> gpui::FileDropEffect {
    let plan = drop_plan(paths, dest, modifiers);
    match (plan.sources.is_empty(), plan.mode) {
        (true, _) => gpui::FileDropEffect::None,
        (false, ClipMode::Cut) => gpui::FileDropEffect::Move,
        (false, ClipMode::Copy) => gpui::FileDropEffect::Copy,
    }
}

/// Makes the cursor over `element` say what a drop of files from outside
/// on `folder` will do. Before, it always said "Copy", even where the drop
/// moved (gpui answered nothing else to Windows), and RML read it as moving
/// being impossible.
pub(crate) fn drop_effect_hint<E: InteractiveElement>(element: E, folder: PathBuf) -> E {
    element.on_drag_move::<ExternalPaths>(move |event, window, cx| {
        if event.bounds.contains(&event.event.position) {
            let effect = drop_effect(event.drag(cx).paths(), &folder, window.modifiers());
            window.set_file_drop_effect(effect);
        }
    })
}

/// The extraction behind a drag out of an archive: written once by its
/// thread, waited for by the drop.
#[derive(Default)]
pub(crate) struct Extraction {
    result: std::sync::Mutex<Option<Result<(), String>>>,
    ended: std::sync::Condvar,
}

impl Extraction {
    fn finish(&self, result: Result<(), String>) {
        if let Ok(mut slot) = self.result.lock() {
            *slot = Some(result);
        }
        self.ended.notify_all();
    }

    /// How it ended, or `None` while it runs.
    fn result(&self) -> Option<Result<(), String>> {
        self.result.lock().ok().and_then(|slot| slot.clone())
    }

    /// Blocks until it ends.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn wait(&self) -> Result<(), String> {
        let Ok(mut slot) = self.result.lock() else {
            return Err("extraction lock poisoned".to_string());
        };
        loop {
            if let Some(result) = slot.as_ref() {
                return result.clone();
            }
            slot = match self.ended.wait(slot) {
                Ok(slot) => slot,
                Err(_) => return Err("extraction lock poisoned".to_string()),
            };
        }
    }
}

/// Makes `element` take files dropped on it into `folder`, tinted while a
/// drag is over it.
pub(crate) fn drop_into<E: InteractiveElement + Styled>(
    element: E,
    folder: &str,
    cx: &mut Context<ExplorerPane>,
) -> E {
    let dest = PathBuf::from(folder);
    drop_effect_hint(element, dest.clone())
        .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(rgb(theme::accent_subtle())))
        .on_drop(cx.listener(move |this, paths: &ExternalPaths, window, cx| {
            this.drop_paths(paths.paths(), dest.clone(), window, cx);
        }))
}

// Whether `a` and `b` are on the same drive, by their prefix (`C:`, or a UNC
// share); compared without case, as Windows does.
fn same_volume(a: &Path, b: &Path) -> bool {
    let prefix = |path: &Path| match path.components().next() {
        Some(std::path::Component::Prefix(prefix)) => {
            Some(prefix.as_os_str().to_string_lossy().to_ascii_lowercase())
        }
        _ => None,
    };
    prefix(a).is_some() && prefix(a) == prefix(b)
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    //! The OLE drag. `unsafe` is confined to this module: the Shell's drag
    //! calls have no safe binding. The data object is the one Explorer itself builds for a
    //! set of files (`BHID_DataObject` on a shell item array), so every target
    //! that takes a drag from Explorer takes this one, with the same formats.

    use std::path::PathBuf;
    use std::sync::Arc;
    use windows::Win32::Foundation::{
        DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND, S_OK,
    };
    use windows::Win32::System::Com::IDataObject;
    use windows::Win32::System::Ole::{
        DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE, IDropSource, IDropSource_Impl,
    };
    use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
    use windows::Win32::UI::WindowsAndMessaging::{IDC_WAIT, LoadCursorW, SetCursor};
    use windows::core::{BOOL, HRESULT, implement};

    use super::Extraction;
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        BHID_DataObject, ILFree, SHCreateShellItemArrayFromIDLists, SHDoDragDrop, SHParseDisplayName,
    };
    use windows::core::HSTRING;

    /// The Properties window Explorer opens for these items: one file's, or
    /// the combined one for several.
    pub fn properties(paths: &[PathBuf]) -> windows::core::Result<()> {
        with_data_object(paths, |data| unsafe { windows::Win32::UI::Shell::SHMultiFileProperties(data, 0) })
    }

    fn with_data_object(
        paths: &[PathBuf],
        run: impl FnOnce(&IDataObject) -> windows::core::Result<()>,
    ) -> windows::core::Result<()> {
        let mut pidls: Vec<*mut ITEMIDLIST> = Vec::with_capacity(paths.len());
        let result = (|| {
            for path in paths {
                let mut pidl = std::ptr::null_mut();
                unsafe { SHParseDisplayName(&HSTRING::from(path.as_os_str()), None, &mut pidl, 0, None)? };
                pidls.push(pidl);
            }
            let items: Vec<*const ITEMIDLIST> = pidls.iter().map(|pidl| pidl.cast_const()).collect();
            let data: IDataObject = unsafe {
                let array = SHCreateShellItemArrayFromIDLists(&items)?;
                array.BindToHandler(None, &BHID_DataObject)?
            };
            run(&data)
        })();
        for pidl in pidls {
            unsafe { ILFree(Some(pidl.cast_const())) };
        }
        result
    }

    pub fn drag(hwnd: i64, paths: &[PathBuf]) -> windows::core::Result<()> {
        let mut pidls: Vec<*mut ITEMIDLIST> = Vec::with_capacity(paths.len());
        let result = (|| {
            for path in paths {
                let mut pidl = std::ptr::null_mut();
                unsafe { SHParseDisplayName(&HSTRING::from(path.as_os_str()), None, &mut pidl, 0, None)? };
                pidls.push(pidl);
            }
            let items: Vec<*const ITEMIDLIST> = pidls.iter().map(|pidl| pidl.cast_const()).collect();
            unsafe {
                let array = SHCreateShellItemArrayFromIDLists(&items)?;
                let data: IDataObject = array.BindToHandler(None, &BHID_DataObject)?;
                // With no drop source of our own the Shell supplies its own,
                // with the drag image and the cursor Explorer shows.
                SHDoDragDrop(
                    Some(HWND(hwnd as *mut _)),
                    &data,
                    None::<&IDropSource>,
                    DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK,
                )?;
            }
            Ok(())
        })();
        for pidl in pidls {
            unsafe { ILFree(Some(pidl.cast_const())) };
        }
        result
    }

    /// The drag of files still being extracted, 7-Zip's way: our own drop
    /// source, which on the button's release holds the drop until
    /// `extraction` ends, and cancels it when the extraction failed. The
    /// data object is still the Shell's, so the drag image and every target
    /// are the same as for any other file.
    pub fn drag_pending(hwnd: i64, paths: &[PathBuf], extraction: &Arc<Extraction>) -> windows::core::Result<()> {
        let source: IDropSource = PendingSource { extraction: extraction.clone() }.into();
        with_data_object(paths, |data| unsafe {
            SHDoDragDrop(
                Some(HWND(hwnd as *mut _)),
                data,
                &source,
                DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK,
            )
            .map(drop)
        })
    }

    #[implement(IDropSource)]
    struct PendingSource {
        extraction: Arc<Extraction>,
    }

    impl IDropSource_Impl for PendingSource_Impl {
        fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
            if escape.as_bool() {
                return DRAGDROP_S_CANCEL;
            }
            if keys.0 & MK_LBUTTON.0 != 0 {
                return S_OK;
            }
            // Released: the target gets the files only once they are whole.
            // The wait cursor, since nothing repaints while this blocks.
            if self.extraction.result().is_none()
                && let Ok(cursor) = unsafe { LoadCursorW(None, IDC_WAIT) }
            {
                unsafe { SetCursor(Some(cursor)) };
            }
            match self.extraction.wait() {
                Ok(()) => DRAGDROP_S_DROP,
                Err(_) => DRAGDROP_S_CANCEL,
            }
        }

        fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::Extraction;

    pub fn drag(_hwnd: i64, _paths: &[PathBuf]) -> Result<(), std::io::Error> {
        Ok(())
    }

    pub fn drag_pending(_hwnd: i64, _paths: &[PathBuf], _extraction: &Arc<Extraction>) -> Result<(), std::io::Error> {
        Ok(())
    }

    pub fn properties(_paths: &[PathBuf]) -> Result<(), std::io::Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::same_volume;
    use std::path::Path;

    // Issue #64: what an archiver extracted to the temp folder for a drag
    // is still there after the archiver deletes its own copy.
    #[test]
    fn files_from_another_programs_temp_folder_outlive_it() {
        let source = tempfile::Builder::new().prefix("Rar$DRa").tempdir_in(std::env::temp_dir()).unwrap();
        let file = source.path().join("leia.txt");
        std::fs::write(&file, "conteúdo").unwrap();
        std::fs::create_dir(source.path().join("pasta")).unwrap();
        std::fs::write(source.path().join("pasta").join("b.txt"), "b").unwrap();
        let dropped = vec![file.clone(), source.path().join("pasta")];

        let held = super::hold_foreign_temp_files(&dropped).expect("temp files are held");
        source.close().unwrap();

        assert_eq!(held[0].file_name(), file.file_name());
        assert_eq!(std::fs::read_to_string(&held[0]).unwrap(), "conteúdo");
        assert_eq!(std::fs::read_to_string(held[1].join("b.txt")).unwrap(), "b");
        assert!(super::hold_foreign_temp_files(&held).is_none(), "held files are not held again");
        assert!(super::hold_foreign_temp_files(&[Path::new(r"C:\Windows\win.ini").to_path_buf()]).is_none());
        if let Some(folder) = held[0].parent() {
            std::fs::remove_dir_all(folder).unwrap();
        }
    }

    #[test]
    fn volumes_compare_by_drive_without_case() {
        assert!(same_volume(Path::new(r"C:\a\b.txt"), Path::new(r"c:\x")));
        assert!(!same_volume(Path::new(r"C:\a"), Path::new(r"D:\a")));
        assert!(!same_volume(Path::new(r"I:\Meu Drive\a.mp4"), Path::new(r"C:\Users")));
    }

    // The case RML reported: a download on the Desktop dropped on a folder
    // of the same drive moves, and the cursor has to say so.
    #[test]
    fn the_cursor_says_what_the_drop_does() {
        use super::drop_effect;
        use gpui::{FileDropEffect, Modifiers};
        use std::path::PathBuf;
        let none = Modifiers::default();
        let shift = Modifiers { shift: true, ..Modifiers::default() };
        let ctrl = Modifiers { control: true, ..Modifiers::default() };
        let download = [PathBuf::from(r"C:\Users\a\Desktop\video.mp4")];
        let same_drive = Path::new(r"C:\Projetos");
        let other_drive = Path::new(r"D:\Projetos");
        assert_eq!(drop_effect(&download, same_drive, none), FileDropEffect::Move);
        assert_eq!(drop_effect(&download, same_drive, ctrl), FileDropEffect::Copy);
        assert_eq!(drop_effect(&download, other_drive, none), FileDropEffect::Copy);
        assert_eq!(drop_effect(&download, other_drive, shift), FileDropEffect::Move);
        // Back into the folder it is in, or a folder into itself: nothing.
        assert_eq!(drop_effect(&download, Path::new(r"C:\Users\a\Desktop"), none), FileDropEffect::None);
        let folder = [PathBuf::from(r"C:\Projetos")];
        assert_eq!(drop_effect(&folder, Path::new(r"C:\Projetos\sub"), none), FileDropEffect::None);
    }
}

/// Opens Explorer's Properties window for `paths` (several share one).
pub(crate) fn show_properties(paths: &[PathBuf]) -> Result<(), String> {
    platform::properties(paths).map_err(|error| error.to_string())
}

/// Drags `paths` out as Explorer would; runs the drag's own loop until the
/// drop, so call it from a task, not inside an update.
pub(crate) fn drag_out(hwnd: i64, paths: &[PathBuf]) -> Result<(), String> {
    platform::drag(hwnd, paths).map_err(|error| error.to_string())
}
