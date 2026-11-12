//! The folder on screen follows the disk: a file another program saves,
//! downloads, deletes or renames there shows up (or goes) without F5.
//!
//! Until 05/11/2026 the listing was read on entering a folder, on F5 and
//! after Prime Files' own operations only, so a file saved by an editor or a
//! browser did not appear — a person who tried it said that alone made them
//! leave the explorer they had used before. Explorer itself watches the
//! folders it shows; so does this, with the system's change notifications
//! (`ReadDirectoryChangesW`, through `notify`), not by polling.
//!
//! Only the folder on screen and the Tree view's open folders are watched,
//! not their subfolders: what is not shown needs no refresh. A save is a
//! burst of events (create, write, rename into place), and a download
//! writes for minutes, so events are gathered for [`SETTLE`] and refreshes
//! kept [`SPACING`] apart; each one is the same refresh as F5, which keeps
//! selection, scroll, open folders and preview.

use std::path::Path;
use std::time::{Duration, Instant};

use gpui::{AppContext as _, BackgroundExecutor, Context, Task, WeakEntity};
use notify::{EventKind, RecursiveMode, Watcher as _};

use super::ExplorerPane;

/// How long a burst of events is gathered before the folder is read.
const SETTLE: Duration = Duration::from_millis(200);
/// The least time between two refreshes of a folder that keeps changing.
const SPACING: Duration = Duration::from_millis(800);
/// How often a refresh put off (an open rename field, a search on screen) is
/// tried again.
const RETRY: Duration = Duration::from_secs(1);
/// How often a folder whose drive sends no notifications is looked at again.
const POLL: Duration = Duration::from_secs(3);

/// The watch on a pane's folders; dropping it stops it.
pub(crate) struct FolderWatch {
    /// The folders asked for, before any was checked on disk.
    folders: Vec<String>,
    /// Set once the background has built it; `None` until then, or when no
    /// folder could be watched.
    watcher: Option<notify::RecommendedWatcher>,
    _task: Task<()>,
    _poll: Option<Task<()>>,
}

impl Drop for FolderWatch {
    // Closing the watched folders' handles can hang as long as opening them
    // on a share that stopped answering ("safe handle killer" in Open
    // Salamander's `snooper.cpp`, OTIMIZACAO.md §2.12): the watcher goes on a
    // thread of its own.
    fn drop(&mut self) {
        if let Some(watcher) = self.watcher.take() {
            std::thread::spawn(move || drop(watcher));
        }
    }
}

impl ExplorerPane {
    /// Watches what is on screen now: the folder, and the folders open in
    /// the Tree view. Called after each listing; a no-op when that set did
    /// not change.
    ///
    /// Nothing here touches the disk on the UI thread. Until 11/11/2026 the
    /// `is_dir()` of each folder, opening the watch and asking the drive's
    /// file system ran here, after every listing: with the folder on a share
    /// that stopped answering, the window froze for 21 s each time
    /// (`docs/PERFORMANCE.md`, pass 13). They run in the background now, and
    /// the watch starts when they are done.
    pub(crate) fn watch_listed_folders(&mut self, cx: &mut Context<Self>) {
        let mut folders: Vec<String> = std::iter::once(self.cwd.clone())
            .chain(self.tree.expanded.iter().cloned())
            // "Este Computador", tags and other shell places are no path;
            // whether a path is a folder is the background's question.
            .filter(|folder| Path::new(folder).is_absolute())
            .collect();
        folders.sort();
        folders.dedup();
        if self.folder_watch.as_ref().is_some_and(|watch| watch.folders == folders) {
            return;
        }
        self.folder_watch = None;
        if folders.is_empty() {
            return;
        }
        let (sender, receiver) = async_channel::unbounded::<()>();
        let sender_for_poll = sender.clone();
        let setup = cx.background_spawn({
            let folders = folders.clone();
            async move { start_watching(folders, sender) }
        });
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let Some((watcher, polled)) = setup.await else {
                return;
            };
            let poll = (!polled.is_empty()).then(|| poll(polled, sender_for_poll, &executor));
            // This task belongs to the watch it fills: a newer watch dropped
            // it, and it never gets here.
            let installed = this.update(cx, |pane, _| match pane.folder_watch.as_mut() {
                Some(watch) => {
                    watch.watcher = Some(watcher);
                    watch._poll = poll;
                    true
                }
                None => {
                    std::thread::spawn(move || drop(watcher));
                    false
                }
            });
            if !matches!(installed, Ok(true)) {
                return;
            }
            let mut last: Option<Instant> = None;
            while receiver.recv().await.is_ok() {
                cx.background_executor().timer(SETTLE).await;
                if let Some(since) = last.map(|last| last.elapsed())
                    && since < SPACING
                {
                    cx.background_executor().timer(SPACING - since).await;
                }
                while receiver.try_recv().is_ok() {}
                loop {
                    let Ok(done) = this.update(cx, |pane, cx| pane.refresh_from_disk(cx)) else {
                        return;
                    };
                    if done {
                        break;
                    }
                    cx.background_executor().timer(RETRY).await;
                }
                last = Some(Instant::now());
            }
        });
        self.folder_watch = Some(FolderWatch { folders, watcher: None, _task: task, _poll: None });
    }

    /// Reads the folder again because the disk changed. Put off (false)
    /// while it would get in the way: a name being typed, search hits on
    /// screen, a listing already under way.
    fn refresh_from_disk(&mut self, cx: &mut Context<Self>) -> bool {
        if self.renaming.is_some() || self.search_results.is_some() || self.listing_pending {
            return false;
        }
        self.reload(cx);
        cx.notify();
        true
    }
}

// Off the UI thread: keeps the folders of `wanted` that are folders on disk,
// watches them, and says which of them must be polled instead. `None` when
// there is nothing to watch.
fn start_watching(
    wanted: Vec<String>,
    sender: async_channel::Sender<()>,
) -> Option<(notify::RecommendedWatcher, Vec<String>)> {
    #[cfg(test)]
    tests::STARTED.lock().unwrap().extend(wanted.iter().cloned());
    let folders: Vec<String> = wanted.into_iter().filter(|folder| Path::new(folder).is_dir()).collect();
    if folders.is_empty() {
        return None;
    }
    let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        let Ok(event) = result else {
            return;
        };
        // Reading a file (a thumbnail, the preview, a sound under the
        // mouse) changes nothing in the listing.
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        if sender.try_send(()).is_err() {
            // The pane is gone; its watcher goes with it.
        }
    });
    let mut watcher = match watcher {
        Ok(watcher) => watcher,
        Err(error) => {
            tracing::warn!("cannot watch {}: {error}", folders[0]);
            return None;
        }
    };
    // A drive that sends no notifications — the virtual drive of Google
    // Drive for desktop (FAT32 to Windows), and some network drives —
    // accepts the watch and then stays silent, so a file saved there from
    // the cloud only showed after F5. Those folders are looked at every few
    // seconds instead; NTFS and ReFS (local disks, OneDrive) do notify, so
    // they cost nothing.
    let mut polled = Vec::new();
    for folder in &folders {
        match watcher.watch(Path::new(folder), RecursiveMode::NonRecursive) {
            Ok(()) if notifies(Path::new(folder)) => {}
            Ok(()) => polled.push(folder.clone()),
            Err(error) => {
                tracing::debug!("cannot watch {folder}: {error}");
                polled.push(folder.clone());
            }
        }
    }
    Some((watcher, polled))
}

// Looks at `folders` every [`POLL`] and signals when what a listing would show
// of them (names, sizes, dates, attributes) changed. The reading runs off the
// UI thread; the first look only records the starting point.
fn poll(folders: Vec<String>, changed: async_channel::Sender<()>, executor: &BackgroundExecutor) -> Task<()> {
    let executor = executor.clone();
    executor.clone().spawn(async move {
        let mut last: Option<u64> = None;
        loop {
            executor.timer(POLL).await;
            let now = folders.iter().fold(0u64, |sum, folder| sum.wrapping_add(fingerprint(Path::new(folder))));
            if last.is_some_and(|last| last != now) && changed.send(()).await.is_err() {
                return;
            }
            last = Some(now);
        }
    })
}

#[allow(clippy::disallowed_methods)]
fn fingerprint(folder: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let Ok(entries) = std::fs::read_dir(folder) else {
        return 0;
    };
    // A sum of per-entry hashes, so the order the drive lists them in does
    // not matter.
    entries.flatten().fold(0u64, |sum, entry| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        entry.file_name().hash(&mut hasher);
        if let Ok(metadata) = entry.metadata() {
            metadata.len().hash(&mut hasher);
            metadata.modified().ok().hash(&mut hasher);
            #[cfg(windows)]
            std::os::windows::fs::MetadataExt::file_attributes(&metadata).hash(&mut hasher);
        }
        sum.wrapping_add(hasher.finish())
    })
}

// Whether the folder's file system sends change notifications reliably.
#[cfg(windows)]
#[allow(unsafe_code)]
fn notifies(folder: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};
    use windows::core::PCWSTR;

    let wide: Vec<u16> = folder.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut root = [0u16; 512];
    if unsafe { GetVolumePathNameW(PCWSTR(wide.as_ptr()), &mut root) }.is_err() {
        return true;
    }
    let mut file_system = [0u16; 64];
    if unsafe { GetVolumeInformationW(PCWSTR(root.as_ptr()), None, None, None, None, Some(&mut file_system)) }
        .is_err()
    {
        return true;
    }
    let length = file_system.iter().position(|&unit| unit == 0).unwrap_or(file_system.len());
    let name = String::from_utf16_lossy(&file_system[..length]);
    matches!(name.as_str(), "NTFS" | "ReFS")
}

#[cfg(not(windows))]
fn notifies(_folder: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use gpui::{AppContext as _, TestAppContext, WindowHandle};
    use gpui_component::input::InputState;
    use gpui_component::resizable::ResizableState;
    use prime_files_services::fs::trash::Ledger as TrashLedger;

    use super::ExplorerPane;

    /// The folders `start_watching` began on — where the disk is touched.
    pub(super) static STARTED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    fn pane(cx: &mut TestAppContext) -> WindowHandle<ExplorerPane> {
        cx.update(gpui_component::init);
        cx.add_window(|window, cx| {
            let resizable = cx.new(|_| ResizableState::default());
            let search_input = cx.new(|cx| InputState::new(window, cx));
            ExplorerPane::new(resizable, search_input, None, TrashLedger::KeptByOs, cx.focus_handle())
        })
    }

    // Checking the folders and opening the watch read the disk, which may not
    // answer for 21 s (a dead share): the call returns with nothing opened,
    // and the watch is in place once the background is done.
    #[gpui::test]
    async fn the_watch_is_opened_off_the_ui_thread(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().unwrap();
        let window = pane(cx);
        let folder = dir.path().to_string_lossy().into_owned();
        let watching = |cx: &mut TestAppContext| {
            window
                .read_with(cx, |pane, _| pane.folder_watch.as_ref().map(|watch| watch.watcher.is_some()))
                .unwrap()
        };
        let touched_during_the_call = window
            .update(cx, |pane, _, cx| {
                pane.cwd = folder.clone();
                pane.watch_listed_folders(cx);
                STARTED.lock().unwrap().contains(&folder)
            })
            .unwrap();
        assert!(!touched_during_the_call, "the folder was checked on the UI thread");
        for _ in 0..200 {
            cx.run_until_parked();
            if watching(cx) == Some(true) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the watch never started: {:?}", watching(cx));
    }

    // How long the UI thread is held when the folder on screen is on a share
    // that stopped answering. Run by hand: it waits on the network.
    // PRIME_FILES_DEAD_SHARE=\\192.0.2.1\share cargo test -p prime-files-pages
    //   --lib -- --ignored --nocapture watch_on_a_dead_share
    #[gpui::test]
    #[ignore = "waits on the network; a measurement, not a check"]
    async fn watch_on_a_dead_share_holds_the_ui_for(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let dead = std::env::var("PRIME_FILES_DEAD_SHARE").unwrap_or_else(|_| r"\\192.0.2.1\share".to_string());
        let window = pane(cx);
        let held = window
            .update(cx, |pane, _, cx| {
                pane.cwd = dead.clone();
                let start = Instant::now();
                pane.watch_listed_folders(cx);
                start.elapsed()
            })
            .unwrap();
        eprintln!("watch_listed_folders on {dead}: UI thread held {:.1} ms", held.as_secs_f64() * 1000.0);
    }
}
