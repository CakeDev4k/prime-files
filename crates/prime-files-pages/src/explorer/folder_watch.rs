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
/// The most: a folder that takes 500 ms to read is read every 10 s while
/// something keeps writing in it, not more seldom.
const MAX_SPACING: Duration = Duration::from_secs(10);
/// How often a refresh put off (an open rename field, a search on screen) is
/// tried again.
const RETRY: Duration = Duration::from_secs(1);
/// How often a folder whose drive sends no notifications is looked at again,
/// at least, and at most.
const POLL: Duration = Duration::from_secs(3);
const MAX_POLL: Duration = Duration::from_secs(30);
/// The least time between two refreshes while the window is behind another:
/// Open Salamander's `MIN_DELAY_BETWEENINACTIVEREFRESHES` (`fileswnb.cpp`,
/// OTIMIZACAO.md §5.9). Whoever is looking at another program still sees a
/// window beside it follow the disk, only less often.
const BEHIND_SPACING: Duration = Duration::from_secs(2);
/// The same for a drive that sends no notifications, where each look reads
/// the whole folder: a Google Drive folder left open behind the browser was
/// read every 3 s all day. Coming to the front looks at once.
const BEHIND_POLL: Duration = Duration::from_secs(10);
const MAX_BEHIND_POLL: Duration = Duration::from_secs(60);
/// How often a poll waiting out the slower pace looks whether the window
/// came to the front.
const POLL_CHECK: Duration = Duration::from_millis(500);
/// How often a watch held quiet by an operation of ours looks whether it
/// ended.
const QUIET_CHECK: Duration = Duration::from_millis(100);

/// The wait before doing again what took `cost`: twenty times as long, so
/// the watch never spends more than ~5% of the time reading, between `least`
/// and `most`. Open Salamander paces its refreshes in the background the same
/// way (`fileswnb.cpp`, 20× the last refresh, 2–10 s; OTIMIZACAO.md §5.9): a
/// folder read in 5 ms follows at once, a share that takes 400 ms waits 8 s.
fn paced(cost: Duration, least: Duration, most: Duration) -> Duration {
    (cost * 20).clamp(least, most)
}

/// The wait between two refreshes after one that took `cost`.
fn refresh_spacing(cost: Duration, in_front: bool) -> Duration {
    paced(cost, if in_front { SPACING } else { BEHIND_SPACING }, MAX_SPACING)
}

/// The wait between two looks at a folder that sends no notifications.
fn poll_wait(cost: Duration, in_front: bool) -> Duration {
    if in_front { paced(cost, POLL, MAX_POLL) } else { paced(cost, BEHIND_POLL, MAX_BEHIND_POLL) }
}

fn is_in_front(in_front: &std::sync::atomic::AtomicBool) -> bool {
    in_front.load(std::sync::atomic::Ordering::Relaxed)
}

/// Prime Files' own operations running on the folder this pane watches.
#[derive(Default)]
struct Quiet {
    running: std::cell::Cell<usize>,
    /// When the last one ended, and the reload it does began.
    ended: std::cell::Cell<Option<Instant>>,
}

/// Held while an operation of ours writes where the pane looks: the events
/// it raises gather into the one reload it does at the end, instead of a
/// refresh every [`SPACING`] — Open Salamander's "suspend mode"
/// (`snooper.cpp`, `BeginSuspendMode`; OTIMIZACAO.md §6.9). Until 11/11/2026
/// a copy of 60 files over 3 s into the open folder read it 6 times; a copy
/// of ten minutes, some 750 (`docs/PERFORMANCE.md`, pass 15).
pub(crate) struct OwnOperation {
    quiet: std::rc::Rc<Quiet>,
    /// The folders it holds quiet for every pane, not only the one that
    /// started it (`BUSY`).
    folders: Vec<String>,
}

impl Drop for OwnOperation {
    fn drop(&mut self) {
        self.quiet.running.set(self.quiet.running.get().saturating_sub(1));
        self.quiet.ended.set(Some(Instant::now()));
        BUSY.with_borrow_mut(|busy| {
            for folder in &self.folders {
                if let Some(count) = busy.get_mut(folder) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        busy.remove(folder);
                    }
                }
            }
        });
    }
}

thread_local! {
    /// Folders an operation of ours is writing in, and how many, for every
    /// pane of the window's thread. Until 12/11/2026 the quiet was the
    /// starting pane's alone: another tab on the same folder read it every
    /// 800 ms through a whole copy (`docs/PERFORMANCE.md`, pass 21).
    static BUSY: std::cell::RefCell<std::collections::HashMap<String, usize>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// A folder as `BUSY` keys it: either separator, any case, no trailing one.
fn busy_key(folder: &str) -> String {
    folder.trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase()
}

/// Whether an operation of ours, from any pane, writes in one of `folders`
/// (already `busy_key`s).
fn any_busy(folders: &[String]) -> bool {
    BUSY.with_borrow(|busy| folders.iter().any(|folder| busy.contains_key(folder)))
}

/// The watch on a pane's folders; dropping it stops it.
pub(crate) struct FolderWatch {
    /// The folders asked for, before any was checked on disk.
    folders: Vec<String>,
    /// Set once the background has built it; `None` until then, or when no
    /// folder could be watched.
    watcher: Option<notify::RecommendedWatcher>,
    quiet: std::rc::Rc<Quiet>,
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
        // Each event carries when it was heard: one heard before an
        // operation of ours ended is covered by the reload that operation
        // does (a logical clock, OTIMIZACAO.md §5.4).
        let (sender, receiver) = async_channel::unbounded::<Instant>();
        let sender_for_poll = sender.clone();
        let quiet = std::rc::Rc::new(Quiet::default());
        let quiet_for_task = quiet.clone();
        let busy_keys: Vec<String> = folders.iter().map(|folder| busy_key(folder)).collect();
        let in_front = self.in_front.clone();
        let setup = cx.background_spawn({
            let folders = folders.clone();
            async move { start_watching(folders, sender) }
        });
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let Some((watcher, polled)) = setup.await else {
                return;
            };
            let poll = (!polled.is_empty()).then(|| poll(polled, sender_for_poll, in_front.clone(), &executor));
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
            let quiet = quiet_for_task;
            let mut last: Option<Instant> = None;
            let mut cost = Duration::ZERO;
            while let Ok(mut heard) = receiver.recv().await {
                cx.background_executor().timer(SETTLE).await;
                // Waited in steps, so coming to the front ends a wait set by
                // the slower pace behind.
                while let Some(since) = last.map(|last| last.elapsed()) {
                    let spacing = refresh_spacing(cost, is_in_front(&in_front));
                    if since >= spacing {
                        break;
                    }
                    cx.background_executor().timer((spacing - since).min(QUIET_CHECK)).await;
                }
                // Ours, or another pane's on one of these folders: that
                // pane reloads itself at the end; this one reads once then,
                // below, for what it heard meanwhile.
                while quiet.running.get() > 0 || any_busy(&busy_keys) {
                    cx.background_executor().timer(QUIET_CHECK).await;
                }
                while let Ok(also) = receiver.try_recv() {
                    heard = heard.max(also);
                }
                if quiet.ended.get().is_some_and(|ended| heard <= ended) {
                    continue;
                }
                loop {
                    let Ok(done) = this.update(cx, |pane, cx| pane.refresh_from_disk(cx)) else {
                        return;
                    };
                    if done {
                        break;
                    }
                    cx.background_executor().timer(RETRY).await;
                }
                // What the read cost sets the pace of the next one, as timed
                // by the read's own thread. Until 12/11/2026 it was timed
                // here, by looking every 25 ms whether the listing was in: a
                // busy UI thread made a 1 ms read look like a second, and the
                // next refresh waited the 10 s most (`docs/PERFORMANCE.md`,
                // pass 17).
                loop {
                    match this.read_with(cx, |pane, _| (!pane.listing_pending).then_some(pane.last_read_cost)) {
                        Ok(Some(took)) => {
                            cost = took;
                            break;
                        }
                        Ok(None) => cx.background_executor().timer(Duration::from_millis(25)).await,
                        Err(_) => return,
                    }
                }
                last = Some(Instant::now());
            }
        });
        self.folder_watch = Some(FolderWatch { folders, watcher: None, quiet, _task: task, _poll: None });
    }

    /// Holds the watch quiet while an operation of ours writes in the
    /// folder; the operation reloads when done and drops what this returns
    /// right before (`run_fs_op_with`). `None` with nothing watched.
    pub(crate) fn begin_own_operation(&self) -> Option<OwnOperation> {
        let watch = self.folder_watch.as_ref()?;
        watch.quiet.running.set(watch.quiet.running.get() + 1);
        let folders: Vec<String> = watch.folders.iter().map(|folder| busy_key(folder)).collect();
        BUSY.with_borrow_mut(|busy| {
            for folder in &folders {
                *busy.entry(folder.clone()).or_insert(0) += 1;
            }
        });
        Some(OwnOperation { quiet: watch.quiet.clone(), folders })
    }

    /// Reads the folder again because the disk changed. Put off (false)
    /// while it would get in the way: a name being typed, search hits on
    /// screen, a listing already under way.
    fn refresh_from_disk(&mut self, cx: &mut Context<Self>) -> bool {
        if self.renaming.is_some() || self.search_results.is_some() || self.listing_pending {
            return false;
        }
        self.reload(cx);
        // A read answered at once and identical to what is shown needs no
        // frame; one still under way notifies when it lands.
        if self.listing_pending || !self.listing_unchanged {
            cx.notify();
        }
        true
    }
}

// Off the UI thread: keeps the folders of `wanted` that are folders on disk,
// watches them, and says which of them must be polled instead. `None` when
// there is nothing to watch.
fn start_watching(
    wanted: Vec<String>,
    sender: async_channel::Sender<Instant>,
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
        if sender.try_send(Instant::now()).is_err() {
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

// Looks at `folders` every [`POLL`] or more and signals when what a listing
// would show of them (names, sizes, dates, attributes) changed. The reading
// runs off the UI thread; the first look only records the starting point.
// Reading a Google Drive folder of thousands of files every 3 s regardless
// would keep a core busy for a folder that is just open: the wait follows
// what the last look cost ([`paced`]), and is longer behind another window
// ([`poll_wait`]).
fn poll(
    folders: Vec<String>,
    changed: async_channel::Sender<Instant>,
    in_front: std::sync::Arc<std::sync::atomic::AtomicBool>,
    executor: &BackgroundExecutor,
) -> Task<()> {
    let executor = executor.clone();
    executor.clone().spawn(async move {
        let mut last: Option<u64> = None;
        let mut looked = Instant::now();
        let mut cost = Duration::ZERO;
        loop {
            loop {
                let wait = poll_wait(cost, is_in_front(&in_front));
                let since = looked.elapsed();
                if since >= wait {
                    break;
                }
                executor.timer((wait - since).min(POLL_CHECK)).await;
            }
            let started = Instant::now();
            let now = folders.iter().fold(0u64, |sum, folder| sum.wrapping_add(fingerprint(Path::new(folder))));
            cost = started.elapsed();
            looked = Instant::now();
            #[cfg(test)]
            tests::LOOKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if last.is_some_and(|last| last != now) && changed.send(Instant::now()).await.is_err() {
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
    /// The looks `poll` took. Only the test below polls: the other tests'
    /// folders are on NTFS, which notifies.
    pub(super) static LOOKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

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

    // Pumps the test's clock along with the real one, so the watcher's
    // timers keep the pace of the writes on real threads.
    fn pump(cx: &mut TestAppContext, real: std::time::Duration) {
        let until = Instant::now() + real;
        while Instant::now() < until {
            cx.executor().advance_clock(std::time::Duration::from_millis(10));
            cx.run_until_parked();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    // A pane on `folder`, listed and watched.
    fn watched_pane(cx: &mut TestAppContext, folder: &std::path::Path) -> WindowHandle<ExplorerPane> {
        let window = pane(cx);
        window
            .update(cx, |pane, _, cx| {
                pane.cwd = folder.to_string_lossy().into_owned();
                pane.reload(cx);
            })
            .unwrap();
        for _ in 0..300 {
            pump(cx, std::time::Duration::from_millis(20));
            let ready = window
                .read_with(cx, |pane, _| {
                    !pane.listing_pending && pane.folder_watch.as_ref().is_some_and(|watch| watch.watcher.is_some())
                })
                .unwrap();
            if ready {
                return window;
            }
        }
        panic!("the folder was never listed and watched");
    }

    // How many times a copy of ours into the folder on screen reads it again:
    // 60 files over ~3 s, then the one reload `run_fs_op_with` does at the
    // end. Measures and prints; the check is in the test after it.
    fn reloads_during_an_own_copy(cx: &mut TestAppContext, quiet: bool) -> u64 {
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().unwrap();
        let window = watched_pane(cx, dir.path());
        let before = window.read_with(cx, |pane, _| pane.listing_generation).unwrap();
        let guard = quiet.then(|| window.update(cx, |pane, _, _| pane.begin_own_operation()).unwrap()).flatten();
        let folder = dir.path().to_path_buf();
        let writer = std::thread::spawn(move || {
            for index in 0..60 {
                std::fs::write(folder.join(format!("copiado-{index:03}.txt")), "x").unwrap();
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
        while !writer.is_finished() {
            pump(cx, std::time::Duration::from_millis(50));
        }
        writer.join().unwrap();
        // What `run_fs_op_with` does when the batch is done.
        drop(guard);
        window.update(cx, |pane, _, cx| pane.reload(cx)).unwrap();
        pump(cx, std::time::Duration::from_secs(3));
        let reloads = window.read_with(cx, |pane, _| pane.listing_generation).unwrap() - before;
        let names = window.read_with(cx, |pane, _| pane.filtered_entries.len()).unwrap();
        assert_eq!(names, 60, "the listing ends with every file");
        eprintln!("own copy of 60 files over ~3 s (quiet: {quiet}): {reloads} reloads");
        reloads
    }

    // The watch's own refreshes while something else writes, for the record
    // (6 before 11/11/2026 for this copy, when ours were not told apart).
    #[gpui::test]
    #[ignore = "a measurement, not a check"]
    async fn measure_reloads_without_quiet(cx: &mut TestAppContext) {
        reloads_during_an_own_copy(cx, false);
    }

    // A copy of ours reads the folder once, at its end, however long it
    // writes there (`docs/PERFORMANCE.md`, pass 15).
    #[gpui::test]
    async fn an_own_copy_reads_the_folder_once(cx: &mut TestAppContext) {
        assert_eq!(reloads_during_an_own_copy(cx, true), 1);
    }

    // Another program writes 120 files over ~6 s; how many refreshes the
    // watch does, and how many of the files are on screen by 4.5 s after the
    // last one.
    fn another_program_writes(cx: &mut TestAppContext, in_front: bool) -> (u64, usize) {
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().unwrap();
        let window = watched_pane(cx, dir.path());
        set_in_front(&window, cx, in_front);
        let before = window.read_with(cx, |pane, _| pane.listing_generation).unwrap();
        let folder = dir.path().to_path_buf();
        let writer = std::thread::spawn(move || {
            for index in 0..120 {
                std::fs::write(folder.join(format!("baixado-{index:03}.txt")), "x").unwrap();
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
        while !writer.is_finished() {
            pump(cx, std::time::Duration::from_millis(50));
        }
        writer.join().unwrap();
        // The last refresh comes within the pace the watch set from what its
        // last read cost, plus room for the read itself. A fixed 4.5 s
        // failed 2 runs in 8 of the whole suite (13/11/2026): with the suite
        // loading every core a read of these 120 names cost over 200 ms, and
        // 20× that is the pace by design. The old fault this test caught
        // (the cost timed on the UI thread) is held by
        // `a_read_is_timed_by_its_own_thread`.
        let cost = window.read_with(cx, |pane, _| pane.last_read_cost).unwrap();
        let promised = super::SETTLE + super::refresh_spacing(cost, in_front) + cost * 4 + std::time::Duration::from_millis(1500);
        let until = Instant::now() + promised.max(std::time::Duration::from_millis(4500));
        while Instant::now() < until && window.read_with(cx, |pane, _| pane.filtered_entries.len()).unwrap() < 120 {
            pump(cx, std::time::Duration::from_millis(50));
        }
        let reloads = window.read_with(cx, |pane, _| pane.listing_generation).unwrap() - before;
        let shown = window.read_with(cx, |pane, _| pane.filtered_entries.len()).unwrap();
        eprintln!("another program, 120 files over ~6 s (in front: {in_front}): {reloads} reloads, {shown} shown");
        (reloads, shown)
    }

    // The pace follows what the read cost on its own thread. Until 12/11/2026
    // it was timed on the UI thread, in 25 ms steps, until the listing was
    // in: a busy UI thread made a 1 ms read look like a second, and the next
    // refresh waited the 10 s most — here, 37 of the 120 files on screen 3 s
    // after the last was written (`docs/PERFORMANCE.md`, pass 17).
    #[gpui::test]
    async fn the_watch_keeps_up_with_another_program_writing(cx: &mut TestAppContext) {
        assert_eq!(another_program_writes(cx, true).1, 120);
    }

    // A refresh that reads exactly what is shown rebuilds and draws nothing;
    // the first difference, even one byte of size, takes the full path
    // (`docs/PERFORMANCE.md`, pass 27).
    #[gpui::test]
    async fn a_refresh_that_reads_the_same_rows_changes_nothing(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..20 {
            std::fs::write(dir.path().join(format!("arquivo-{index:02}.txt")), "x").unwrap();
        }
        let window = pane(cx);
        let reload = |cx: &mut TestAppContext| {
            window.update(cx, |pane, _, cx| pane.reload(cx)).unwrap();
            cx.run_until_parked();
            window.read_with(cx, |pane, _| (pane.listing_unchanged, pane.filtered_entries.len())).unwrap()
        };
        window
            .update(cx, |pane, _, _| pane.cwd = dir.path().to_string_lossy().into_owned())
            .unwrap();
        assert_eq!(reload(cx), (false, 20), "a visit is never unchanged");
        assert_eq!(reload(cx), (true, 20));
        std::fs::write(dir.path().join("arquivo-03.txt"), "xy").unwrap();
        assert_eq!(reload(cx), (false, 20), "a size changed");
        std::fs::write(dir.path().join("novo.txt"), "x").unwrap();
        assert_eq!(reload(cx), (false, 21), "a name appeared");
        assert_eq!(reload(cx), (true, 21));
    }

    // The cost is what the read took on its thread, not how long the UI
    // thread took to take the answer in: here it is busy 300 ms first.
    #[gpui::test]
    async fn a_read_is_timed_by_its_own_thread(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().unwrap();
        for index in 0..50 {
            std::fs::write(dir.path().join(format!("arquivo-{index:02}.txt")), "x").unwrap();
        }
        let window = pane(cx);
        window
            .update(cx, |pane, _, cx| {
                pane.cwd = dir.path().to_string_lossy().into_owned();
                // The background path, as on a slow drive.
                pane.listing_wait = std::time::Duration::ZERO;
                pane.reload(cx);
                std::thread::sleep(std::time::Duration::from_millis(300));
            })
            .unwrap();
        for _ in 0..300 {
            cx.run_until_parked();
            if !window.read_with(cx, |pane, _| pane.listing_pending).unwrap() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let cost = window.read_with(cx, |pane, _| pane.last_read_cost).unwrap();
        assert!(!cost.is_zero(), "the read's cost never reached the pane");
        assert!(cost < std::time::Duration::from_millis(300), "the busy UI thread was counted: {cost:?}");
    }

    // For the record: the refreshes in front and behind another window.
    #[gpui::test]
    #[ignore = "a measurement, not a check"]
    async fn measure_reloads_in_front_and_behind(cx: &mut TestAppContext) {
        for in_front in [true, false, true, false] {
            another_program_writes(cx, in_front);
        }
    }

    // Every batch goes through `run_fs_op_with`: it holds the watch quiet
    // until it is done, and lets go.
    #[gpui::test]
    async fn a_file_operation_holds_the_watch_quiet_until_done(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().unwrap();
        let window = watched_pane(cx, dir.path());
        let running = |cx: &mut TestAppContext| {
            window.read_with(cx, |pane, _| pane.folder_watch.as_ref().unwrap().quiet.running.get()).unwrap()
        };
        let folder = dir.path().to_path_buf();
        window
            .update(cx, |pane, _, cx| {
                pane.run_fs_op(1, "ok".into(), cx, move || {
                    std::fs::write(folder.join("novo.txt"), "x").unwrap();
                    super::super::file_ops::OpReport::default()
                })
            })
            .unwrap();
        assert_eq!(running(cx), 1, "the batch did not hold the watch");
        pump(cx, std::time::Duration::from_millis(500));
        assert_eq!(running(cx), 0, "the batch never let the watch go");
    }

    // A folder that takes 500 ms to read is read every 10 s while another
    // program keeps writing there (20× its cost, at most 10 s), not every
    // 0.8 s: 1 read in these ~8 s, against 3 with the cost taken for zero.
    // Until 12/11/2026 no test held this: a watch that took the cost for
    // zero passed them all, since temp folders read in ~1 ms.
    #[gpui::test]
    async fn a_slow_folder_is_read_at_the_pace_of_its_cost(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().unwrap();
        let window = watched_pane(cx, dir.path());
        crate::explorer::navigation::make_read_slow(&dir.path().to_string_lossy(), std::time::Duration::from_millis(500));
        // Read in the background, as in the app: waited for on the UI
        // thread, the slow read would also hold the test's clock.
        window.update(cx, |pane, _, _| pane.listing_wait = std::time::Duration::ZERO).unwrap();
        let before = window.read_with(cx, |pane, _| pane.listing_generation).unwrap();
        let folder = dir.path().to_path_buf();
        let writer = std::thread::spawn(move || {
            for index in 0..160 {
                std::fs::write(folder.join(format!("baixado-{index:03}.txt")), "x").unwrap();
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
        while !writer.is_finished() {
            pump(cx, std::time::Duration::from_millis(50));
        }
        writer.join().unwrap();
        let reloads = window.read_with(cx, |pane, _| pane.listing_generation).unwrap() - before;
        let cost = window.read_with(cx, |pane, _| pane.last_read_cost).unwrap();
        eprintln!("slow folder, ~8 s of writes: {reloads} reloads, last cost {cost:?}");
        assert!(cost >= std::time::Duration::from_millis(500), "the slow read was not timed: {cost:?}");
        assert!(reloads <= 2, "a 500 ms folder was read {reloads} times in ~8 s");
    }

    // Another tab on the folder an operation of ours writes in waits it out
    // too, and reads once at the end; a tab on another folder is not held.
    #[gpui::test]
    async fn another_tab_on_the_folder_waits_for_our_copy(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let ours = watched_pane(cx, dir.path());
        let other = watched_pane(cx, dir.path());
        let apart = watched_pane(cx, elsewhere.path());
        let generation = |window: &WindowHandle<ExplorerPane>, cx: &mut TestAppContext| {
            window.read_with(cx, |pane, _| pane.listing_generation).unwrap()
        };
        let other_before = generation(&other, cx);
        let apart_before = generation(&apart, cx);
        let guard = ours.update(cx, |pane, _, _| pane.begin_own_operation()).unwrap();
        assert!(guard.is_some());
        let (folder, other_folder) = (dir.path().to_path_buf(), elsewhere.path().to_path_buf());
        let writer = std::thread::spawn(move || {
            for index in 0..40 {
                std::fs::write(folder.join(format!("copiado-{index:03}.txt")), "x").unwrap();
                if index == 0 {
                    std::fs::write(other_folder.join("de-outro-programa.txt"), "x").unwrap();
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
        while !writer.is_finished() {
            pump(cx, std::time::Duration::from_millis(50));
        }
        writer.join().unwrap();
        assert_eq!(generation(&other, cx), other_before, "the other tab read the folder during our copy");
        assert!(generation(&apart, cx) > apart_before, "a tab on another folder was held too");
        drop(guard);
        ours.update(cx, |pane, _, cx| pane.reload(cx)).unwrap();
        let until = Instant::now() + std::time::Duration::from_secs(3);
        while Instant::now() < until && other.read_with(cx, |pane, _| pane.filtered_entries.len()).unwrap() < 40 {
            pump(cx, std::time::Duration::from_millis(50));
        }
        pump(cx, std::time::Duration::from_secs(1));
        assert_eq!(generation(&other, cx) - other_before, 1, "the other tab reads once, at the end");
        assert_eq!(other.read_with(cx, |pane, _| pane.filtered_entries.len()).unwrap(), 40);
    }

    // A phone's operations run on a thread of their own, not through
    // `run_fs_op_with`; a copy from the phone into the folder on screen
    // holds the watch quiet the same way.
    #[gpui::test]
    async fn a_device_operation_holds_the_watch_quiet_until_done(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().unwrap();
        let window = watched_pane(cx, dir.path());
        let running = |cx: &mut TestAppContext| {
            window.read_with(cx, |pane, _| pane.folder_watch.as_ref().unwrap().quiet.running.get()).unwrap()
        };
        let (release, released) = std::sync::mpsc::channel::<()>();
        let folder = dir.path().to_path_buf();
        window
            .update(cx, |pane, _, cx| {
                pane.run_device_op(
                    move |_| {
                        std::fs::write(folder.join("do-celular.jpg"), "x").map_err(|error| error.to_string())?;
                        released.recv().map_err(|error| error.to_string())
                    },
                    None,
                    cx,
                )
            })
            .unwrap();
        pump(cx, std::time::Duration::from_millis(300));
        assert_eq!(running(cx), 1, "the phone's copy did not hold the watch");
        release.send(()).unwrap();
        pump(cx, std::time::Duration::from_millis(500));
        assert_eq!(running(cx), 0, "the phone's copy never let the watch go");
        assert_eq!(names(&window, cx), vec!["do-celular.jpg".to_string()], "the reload at the end shows the file");
    }

    #[test]
    fn the_watch_waits_twenty_times_what_a_read_cost() {
        use super::{MAX_SPACING, SPACING, paced};
        use std::time::Duration;
        assert_eq!(paced(Duration::from_millis(5), SPACING, MAX_SPACING), SPACING, "a quick folder follows at once");
        assert_eq!(paced(Duration::from_millis(100), SPACING, MAX_SPACING), Duration::from_secs(2));
        assert_eq!(paced(Duration::from_secs(2), SPACING, MAX_SPACING), MAX_SPACING, "never longer than the most");
    }

    #[test]
    fn behind_another_window_the_watch_keeps_a_slower_pace() {
        use super::{
            BEHIND_POLL, BEHIND_SPACING, MAX_BEHIND_POLL, MAX_POLL, MAX_SPACING, POLL, SPACING, poll_wait,
            refresh_spacing,
        };
        use std::time::Duration;
        let quick = Duration::from_millis(5);
        assert_eq!(refresh_spacing(quick, true), SPACING);
        assert_eq!(refresh_spacing(quick, false), BEHIND_SPACING);
        assert_eq!(refresh_spacing(Duration::from_secs(2), false), MAX_SPACING, "the most is the same");
        assert_eq!(poll_wait(quick, true), POLL);
        assert_eq!(poll_wait(quick, false), BEHIND_POLL);
        assert_eq!(poll_wait(Duration::from_millis(400), false), Duration::from_secs(10));
        assert_eq!(poll_wait(Duration::from_secs(2), true), MAX_POLL);
        assert_eq!(poll_wait(Duration::from_secs(5), false), MAX_BEHIND_POLL);
    }

    fn set_in_front(window: &WindowHandle<ExplorerPane>, cx: &mut TestAppContext, in_front: bool) {
        window
            .update(cx, |pane, _, _| pane.in_front.store(in_front, std::sync::atomic::Ordering::Relaxed))
            .unwrap();
    }

    fn names(window: &WindowHandle<ExplorerPane>, cx: &mut TestAppContext) -> Vec<String> {
        window.read_with(cx, |pane, _| pane.filtered_entries.iter().map(|entry| entry.name.clone()).collect()).unwrap()
    }

    // Behind another window, a second save right after a refresh waits the
    // 2 s floor instead of 0.8 s; coming to the front shows it at once.
    #[gpui::test]
    async fn behind_another_window_refreshes_wait_longer_until_it_comes_to_the_front(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().unwrap();
        let window = watched_pane(cx, dir.path());
        set_in_front(&window, cx, false);
        std::fs::write(dir.path().join("a.txt"), "A").unwrap();
        for _ in 0..300 {
            pump(cx, std::time::Duration::from_millis(20));
            if names(&window, cx) == ["a.txt"] {
                break;
            }
        }
        assert_eq!(names(&window, cx), ["a.txt"], "the first save shows behind, too");
        std::fs::write(dir.path().join("b.txt"), "B").unwrap();
        pump(cx, std::time::Duration::from_millis(1200));
        assert_eq!(names(&window, cx), ["a.txt"], "behind, the second refresh came before the 2 s floor");
        set_in_front(&window, cx, true);
        pump(cx, std::time::Duration::from_millis(400));
        assert_eq!(names(&window, cx), ["a.txt", "b.txt"], "coming to the front did not refresh at once");
    }

    // A folder that sends no notifications is not read every 3 s behind
    // another window; coming to the front, it is looked at at once.
    #[gpui::test]
    async fn behind_another_window_a_polled_folder_is_looked_at_less(cx: &mut TestAppContext) {
        use std::sync::atomic::{AtomicBool, Ordering};
        cx.executor().allow_parking();
        let dir = tempfile::tempdir().unwrap();
        let in_front = std::sync::Arc::new(AtomicBool::new(false));
        let (sender, _receiver) = async_channel::unbounded();
        LOOKS.store(0, Ordering::Relaxed);
        let _poll = super::poll(
            vec![dir.path().to_string_lossy().into_owned()],
            sender,
            in_front.clone(),
            &cx.executor(),
        );
        let advance = |cx: &mut TestAppContext, real: std::time::Duration| {
            let until = Instant::now() + real;
            while Instant::now() < until {
                cx.executor().advance_clock(std::time::Duration::from_millis(10));
                cx.executor().run_until_parked();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        advance(cx, std::time::Duration::from_millis(4000));
        assert_eq!(LOOKS.load(Ordering::Relaxed), 0, "behind, it looked before the 10 s floor");
        in_front.store(true, Ordering::Relaxed);
        advance(cx, std::time::Duration::from_millis(800));
        assert_eq!(LOOKS.load(Ordering::Relaxed), 1, "coming to the front did not look at once");
    }

    // The window losing focus is what tells the watch it is behind.
    #[gpui::test]
    async fn the_pane_knows_when_its_window_goes_behind(cx: &mut TestAppContext) {
        let window = pane(cx);
        let mut cx = gpui::VisualTestContext::from_window(window.into(), cx);
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        let in_front = |cx: &mut gpui::VisualTestContext| {
            window.read_with(cx, |pane, _| pane.in_front.load(std::sync::atomic::Ordering::Relaxed)).unwrap()
        };
        assert!(window.read_with(&cx, |pane, _| pane.activation_watch.is_some()).unwrap(), "never drawn");
        assert!(in_front(&mut cx), "a window just opened is in front");
        cx.deactivate_window();
        assert!(!in_front(&mut cx), "the pane missed the window going behind");
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
