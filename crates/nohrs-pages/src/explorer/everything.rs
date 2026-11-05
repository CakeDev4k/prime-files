//! "Everything" mode: the bar under the listing searches every name on the
//! computer's fixed drives as you type, the way voidtools' Everything does.
//!
//! Everything reads the NTFS master file table, which needs administrator
//! rights; Prime Files installs per user, so it walks the drives instead and
//! keeps what it found as a flat table of names in memory. A search is then a
//! scan of that table on every core — a few tens of milliseconds for a
//! million names — and never touches the disk except to fill in size and date
//! for the hits shown.
//!
//! The table is saved to the cache folder after each walk and read back the
//! first time the mode is used, so a search answers at once while a fresh
//! walk runs behind it when the saved one is older than [`STALE_AFTER`].
//!
//! Each drive is a segment of its own, published the moment its walk ends:
//! a refresh replaces one drive at a time instead of the whole. The very
//! first walk is searchable while it runs: a search reads the drive being
//! walked under its lock, which holds the walkers for the few milliseconds
//! a scan takes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use gpui::{App, AppContext as _, Context, Global, Subscription, Task, WeakEntity};
use nohrs_core::telemetry::LogErr;
use nohrs_services::fs::listing::FileEntryDto;

use super::ExplorerPane;
use super::quick_search::DeepSearch;

/// A saved table older than this is walked again when the mode opens. A
/// full walk of a busy system drive measured 14 minutes (2.5 million names
/// on this machine's C:, where every folder opened passes the system's file
/// filters; D: walked 88 thousand in under a second), so refreshing more
/// often would keep the disk busy for little.
const STALE_AFTER: Duration = Duration::from_secs(60 * 60);
/// Hits handed to the listing. Past this a search is too broad to read
/// through anyway, and each hit costs a metadata read.
const MAX_RESULTS: usize = 5_000;
/// The first hits go out in a small batch so the listing fills at once.
const FIRST_BATCH: usize = 200;
const BATCH: usize = 1_000;
const NO_PARENT: u32 = u32::MAX;
const FILE_MAGIC: &[u8; 8] = b"PFEVRY02";

#[derive(Clone, Copy)]
struct Entry {
    parent: u32,
    name: u32,
    lower: u32,
    name_len: u16,
    lower_len: u16,
    dir: bool,
}

/// Every name on the indexed drives, a segment per drive.
pub(crate) struct Index {
    segments: Vec<Arc<Segment>>,
    /// When the walk that produced it finished.
    pub built: SystemTime,
}

impl Index {
    /// How many names it holds.
    pub fn len(&self) -> usize {
        self.segments.iter().map(|segment| segment.entries.len()).sum()
    }
}

/// One drive's names.
struct Segment {
    names: String,
    // The names lowercased, apart: lowercasing can change a name's length,
    // and matching against a lowered copy keeps the scan a plain `contains`.
    lower: String,
    entries: Vec<Entry>,
}

impl Segment {
    /// The drive's own name ("C:"), its first entry.
    fn root(&self) -> &str {
        self.entries.first().map_or("", |entry| self.name(entry))
    }

    fn name(&self, entry: &Entry) -> &str {
        let start = entry.name as usize;
        self.names.get(start..start + entry.name_len as usize).unwrap_or("")
    }

    fn lower(&self, entry: &Entry) -> &str {
        let start = entry.lower as usize;
        self.lower.get(start..start + entry.lower_len as usize).unwrap_or("")
    }

    fn path(&self, index: u32) -> String {
        let mut parts = Vec::new();
        let mut current = index;
        // A broken table cannot loop forever: no path is deeper than the
        // table is long.
        while current != NO_PARENT && parts.len() <= self.entries.len() {
            let Some(entry) = self.entries.get(current as usize) else {
                break;
            };
            parts.push(self.name(entry));
            current = entry.parent;
        }
        parts.reverse();
        let mut path = parts.join("\\");
        // A drive alone is "C:\", not "C:".
        if parts.len() == 1 {
            path.push('\\');
        }
        path
    }

    /// (rank, name length, position) of every entry matching `query`.
    fn search(&self, query: &Query, cancel: &AtomicBool) -> Vec<(u8, u16, u32)> {
        let threads = std::thread::available_parallelism().map_or(4, usize::from).clamp(1, 16);
        let chunk = self.entries.len().div_ceil(threads).max(1);
        std::thread::scope(|scope| {
            let workers: Vec<_> = self
                .entries
                .chunks(chunk)
                .enumerate()
                .map(|(part, entries)| {
                    scope.spawn(move || {
                        let mut found = Vec::new();
                        for (offset, entry) in entries.iter().enumerate() {
                            if offset % 65_536 == 0 && cancel.load(Ordering::Relaxed) {
                                return found;
                            }
                            let index = (part * chunk + offset) as u32;
                            if let Some(rank) = query.rank(self, index, entry) {
                                found.push((rank, entry.name_len, index));
                            }
                        }
                        found
                    })
                })
                .collect();
            workers.into_iter().flat_map(|worker| worker.join().unwrap_or_default()).collect()
        })
    }
}

/// What was typed, split into terms that must all match.
struct Query {
    terms: Vec<Term>,
    // The whole query lowered, to rank exact and prefix name matches.
    whole: String,
}

enum Term {
    /// Part of the name.
    Name(String),
    /// Part of the full path (the term had a `\` or `/`).
    Path(String),
    /// A wildcard pattern over the whole name (`*.wav`).
    Pattern(Vec<char>),
}

impl Query {
    fn parse(text: &str) -> Option<Query> {
        let terms: Vec<Term> = text
            .split_whitespace()
            .map(|term| {
                let lower = term.to_lowercase();
                if lower.contains('*') || lower.contains('?') {
                    Term::Pattern(lower.chars().collect())
                } else if lower.contains('\\') || lower.contains('/') {
                    Term::Path(lower.replace('/', "\\"))
                } else {
                    Term::Name(lower)
                }
            })
            .collect();
        if terms.is_empty() {
            return None;
        }
        Some(Query { terms, whole: text.trim().to_lowercase() })
    }

    fn rank(&self, index: &Segment, position: u32, entry: &Entry) -> Option<u8> {
        let name = index.lower(entry);
        // Name terms first: building a path walks the parents, so it is done
        // only for entries every name term already let through.
        let names_match = self.terms.iter().all(|term| match term {
            Term::Name(part) => name.contains(part.as_str()),
            Term::Pattern(pattern) => wildcard(pattern, name),
            Term::Path(_) => true,
        });
        if !names_match {
            return None;
        }
        let path_terms: Vec<&str> = self
            .terms
            .iter()
            .filter_map(|term| match term {
                Term::Path(part) => Some(part.as_str()),
                _ => None,
            })
            .collect();
        if !path_terms.is_empty() {
            let path = index.path(position).to_lowercase();
            if !path_terms.iter().all(|part| path.contains(part)) {
                return None;
            }
        }
        Some(if name == self.whole {
            0
        } else if name.starts_with(&self.whole) {
            1
        } else {
            2
        })
    }
}

// `*` any run, `?` one character, over the whole name.
fn wildcard(pattern: &[char], name: &str) -> bool {
    let name: Vec<char> = name.chars().collect();
    let (mut p, mut n) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, n));
                p += 1;
            }
            Some(&c) if c == '?' || name.get(n) == Some(&c) => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((star_p, star_n)) => {
                    p = star_p + 1;
                    n = star_n + 1;
                    star = Some((star_p, star_n + 1));
                }
                None => return false,
            },
        }
    }
    pattern.get(p..).is_some_and(|rest| rest.iter().all(|&c| c == '*'))
}

// ---------------------------------------------------------------------------
// Walking the drives

impl Segment {
    fn empty() -> Self {
        Self { names: String::new(), lower: String::new(), entries: Vec::new() }
    }

    fn push(&mut self, parent: u32, name: &str, dir: bool) -> u32 {
        // A name longer than this is not a real Windows name (255 UTF-16
        // units at most); clipping keeps the table's lengths in 16 bits.
        let name = clip(name, u16::MAX as usize);
        let lowered = name.to_lowercase();
        let lowered = clip(&lowered, u16::MAX as usize);
        let entry = Entry {
            parent,
            name: self.names.len() as u32,
            lower: self.lower.len() as u32,
            name_len: name.len() as u16,
            lower_len: lowered.len() as u16,
            dir,
        };
        self.names.push_str(name);
        self.lower.push_str(lowered);
        self.entries.push(entry);
        (self.entries.len() - 1) as u32
    }
}

fn clip(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

struct Walk {
    // Folders waiting to be read, and how many workers are reading one.
    queue: Mutex<(Vec<(PathBuf, u32)>, usize)>,
    wake: Condvar,
    out: Arc<Mutex<Segment>>,
    progress: Arc<AtomicUsize>,
}

/// Walks every fixed drive, one after the other: each drive's table, while
/// it fills, goes to `walking`, and each finished drive's segment (with those
/// before it) to `finished`. `progress` counts names found so far.
fn build(
    progress: Arc<AtomicUsize>,
    mut walking: impl FnMut(Arc<Mutex<Segment>>),
    mut finished: impl FnMut(Vec<Arc<Segment>>),
) -> Index {
    let started = Instant::now();
    let mut segments = Vec::new();
    for drive in platform::fixed_drives() {
        let table = Arc::new(Mutex::new(Segment::empty()));
        walking(table.clone());
        let segment = Arc::new(walk_drive(&drive, table, progress.clone()));
        tracing::info!(
            drive = %drive,
            entries = segment.entries.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "everything drive indexed"
        );
        segments.push(segment);
        finished(segments.clone());
    }
    Index { segments, built: SystemTime::now() }
}

fn walk_drive(drive: &str, table: Arc<Mutex<Segment>>, progress: Arc<AtomicUsize>) -> Segment {
    let root = lock(&table).push(NO_PARENT, drive.trim_end_matches('\\'), true);
    let walk = Arc::new(Walk {
        queue: Mutex::new((vec![(PathBuf::from(drive), root)], 0)),
        wake: Condvar::new(),
        out: table,
        progress,
    });
    // Reading folders waits on the disk more than on the processor, so more
    // workers than cores keep an SSD busy.
    let workers = std::thread::available_parallelism().map_or(4, usize::from).clamp(2, 12);
    let handles: Vec<_> = (0..workers)
        .filter_map(|_| {
            let walk = walk.clone();
            std::thread::Builder::new()
                .name("nohrs-everything-walk".into())
                .spawn(move || worker(&walk))
                .log_err()
        })
        .collect();
    for handle in handles {
        if handle.join().is_err() {
            tracing::error!("an index walker panicked");
        }
    }
    std::mem::replace(&mut *lock(&walk.out), Segment::empty())
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn worker(walk: &Walk) {
    loop {
        let job = {
            let mut queue = lock(&walk.queue);
            loop {
                if let Some(job) = queue.0.pop() {
                    queue.1 += 1;
                    break Some(job);
                }
                if queue.1 == 0 {
                    break None;
                }
                queue = walk.wake.wait(queue).unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        let Some((folder, id)) = job else {
            walk.wake.notify_all();
            return;
        };
        // Read outside every lock: this is the slow part.
        let mut found = Vec::new();
        if let Ok(read) = std::fs::read_dir(&folder) {
            for entry in read.filter_map(Result::ok) {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let name = entry.file_name().to_string_lossy().into_owned();
                // Links and junctions are named but not followed: one that
                // points at a parent would walk forever, and the rest point at
                // folders the walk reaches anyway. A cloud folder not yet on
                // the disk (OneDrive's "online only") is named but not read
                // either: listing it makes the sync app fetch it from the
                // network, which held a walk to a few hundred names a second.
                let descend = kind.is_dir() && !kind.is_symlink() && !skipped(&name) && !in_the_cloud(&entry);
                found.push((name, kind.is_dir(), descend));
            }
        }
        let mut children = Vec::new();
        {
            let mut out = lock(&walk.out);
            for (name, dir, descend) in &found {
                let child = out.push(id, name, *dir);
                if *descend {
                    children.push((folder.join(name), child));
                }
            }
        }
        walk.progress.fetch_add(found.len(), Ordering::Relaxed);
        // The queue is taken from its end, and the drive's root lists
        // Windows after Users: without this the first minutes went to
        // C:\Windows, and a search during the first walk found nothing of
        // the person's own files until it was done.
        if id == 0 {
            children.sort_by_key(|(path, _)| walk_priority(path));
        }
        let mut queue = lock(&walk.queue);
        queue.0.extend(children);
        queue.1 -= 1;
        drop(queue);
        walk.wake.notify_all();
    }
}

// Higher is walked sooner: people look for their own files first, and the
// system's folders are the largest and least searched.
fn walk_priority(path: &Path) -> u8 {
    let name = path.file_name().map(|name| name.to_string_lossy().to_lowercase()).unwrap_or_default();
    match name.as_str() {
        "users" => 3,
        "windows" | "programdata" | "$recycle.bin" | "system volume information" => 0,
        name if name.starts_with("program files") => 1,
        _ => 2,
    }
}

#[cfg(windows)]
fn in_the_cloud(entry: &std::fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
    const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x40000;
    const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x400000;
    // On Windows a directory entry's metadata comes from the listing
    // itself: no extra trip to the disk.
    entry.metadata().is_ok_and(|metadata| {
        metadata.file_attributes()
            & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
            != 0
    })
}

#[cfg(not(windows))]
fn in_the_cloud(_entry: &std::fs::DirEntry) -> bool {
    false
}

// Folders whose contents nobody searches for and that only answer "access
// denied" or hold copies of what is elsewhere.
fn skipped(name: &str) -> bool {
    name.eq_ignore_ascii_case("$Recycle.Bin")
        || name.eq_ignore_ascii_case("System Volume Information")
        || name.eq_ignore_ascii_case("$WinREAgent")
}

// ---------------------------------------------------------------------------
// The saved table

fn index_file() -> PathBuf {
    nohrs_core::config::paths::cache_dir().join("everything.idx")
}

fn save(index: &Index, path: &Path) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    let mut file = std::io::BufWriter::new(std::fs::File::create(&temporary)?);
    let built = index.built.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    file.write_all(FILE_MAGIC)?;
    file.write_all(&built.to_le_bytes())?;
    file.write_all(&(index.segments.len() as u64).to_le_bytes())?;
    for segment in &index.segments {
        file.write_all(&(segment.entries.len() as u64).to_le_bytes())?;
        file.write_all(&(segment.names.len() as u64).to_le_bytes())?;
        file.write_all(&(segment.lower.len() as u64).to_le_bytes())?;
        for entry in &segment.entries {
            file.write_all(&entry.parent.to_le_bytes())?;
            file.write_all(&entry.name.to_le_bytes())?;
            file.write_all(&entry.lower.to_le_bytes())?;
            file.write_all(&entry.name_len.to_le_bytes())?;
            file.write_all(&entry.lower_len.to_le_bytes())?;
            file.write_all(&[u8::from(entry.dir)])?;
        }
        file.write_all(segment.names.as_bytes())?;
        file.write_all(segment.lower.as_bytes())?;
    }
    file.into_inner().map_err(|error| error.into_error())?.sync_all()?;
    std::fs::rename(&temporary, path)
}

const ENTRY_BYTES: usize = 17;

fn number(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at.checked_add(8)?)?.try_into().ok()?))
}

fn load(path: &Path) -> Option<Index> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.get(..8)? != FILE_MAGIC {
        return None;
    }
    let built = SystemTime::UNIX_EPOCH + Duration::from_secs(number(&bytes, 8)?);
    let count = usize::try_from(number(&bytes, 16)?).ok()?;
    let mut at = 24usize;
    let mut segments = Vec::new();
    for _ in 0..count {
        let (segment, next) = load_segment(&bytes, at)?;
        segments.push(Arc::new(segment));
        at = next;
    }
    (at == bytes.len()).then_some(Index { segments, built })
}

// The segment starting at `at`, and where the next one starts.
fn load_segment(bytes: &[u8], at: usize) -> Option<(Segment, usize)> {
    let count = usize::try_from(number(bytes, at)?).ok()?;
    let names_len = usize::try_from(number(bytes, at + 8)?).ok()?;
    let lower_len = usize::try_from(number(bytes, at + 16)?).ok()?;
    let entries_start = at.checked_add(24)?;
    let entries_end = entries_start.checked_add(count.checked_mul(ENTRY_BYTES)?)?;
    let names_end = entries_end.checked_add(names_len)?;
    let lower_end = names_end.checked_add(lower_len)?;
    let mut entries = Vec::with_capacity(count);
    for raw in bytes.get(entries_start..entries_end)?.chunks_exact(ENTRY_BYTES) {
        let word = |at: usize| u32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]]);
        let half = |at: usize| u16::from_le_bytes([raw[at], raw[at + 1]]);
        let entry = Entry {
            parent: word(0),
            name: word(4),
            lower: word(8),
            name_len: half(12),
            lower_len: half(14),
            dir: raw[16] != 0,
        };
        // Every slice the table hands out must land inside its strings.
        if entry.name as usize + entry.name_len as usize > names_len
            || entry.lower as usize + entry.lower_len as usize > lower_len
            || (entry.parent != NO_PARENT && entry.parent as usize >= count)
        {
            return None;
        }
        entries.push(entry);
    }
    let names = String::from_utf8(bytes.get(entries_end..names_end)?.to_vec()).ok()?;
    let lower = String::from_utf8(bytes.get(names_end..lower_end)?.to_vec()).ok()?;
    let segment = Segment { names, lower, entries };
    // A slice cut inside a character would come back empty; one such entry
    // means the file is not what this version wrote.
    let consistent = segment.entries.iter().all(|entry| {
        segment.names.is_char_boundary(entry.name as usize) && segment.lower.is_char_boundary(entry.lower as usize)
    });
    consistent.then_some((segment, lower_end))
}

// ---------------------------------------------------------------------------
// The app-wide state

/// The index every pane searches, and the walk refreshing it.
#[derive(Default)]
pub(crate) struct Everything {
    pub index: Option<Arc<Index>>,
    /// Names found so far by the walk in progress.
    pub building: Option<Arc<AtomicUsize>>,
    // The drive being walked, searchable while it fills.
    walking: Option<Arc<Mutex<Segment>>>,
    loading: bool,
    /// Bumped each time `index` is replaced, so panes rerun their search.
    pub generation: u64,
    _task: Option<Task<()>>,
}

impl Global for Everything {}

impl Everything {
    // Whether the drive being walked is missing from the index, so searches
    // read the walk itself.
    fn walking_is_new(&self) -> bool {
        let Some(table) = &self.walking else {
            return false;
        };
        let root = lock(table).root().to_string();
        !self.index.as_ref().is_some_and(|index| index.segments.iter().any(|segment| segment.root() == root))
    }
}

/// What the settings page shows about the index.
pub(crate) struct Status {
    pub entries: Option<usize>,
    pub built: Option<SystemTime>,
    pub building: Option<usize>,
}

/// The index's state, for display.
pub(crate) fn status(cx: &App) -> Status {
    let everything = cx.try_global::<Everything>();
    Status {
        entries: everything.and_then(|state| state.index.as_ref()).map(|index| index.len()),
        built: everything.and_then(|state| state.index.as_ref()).map(|index| index.built),
        building: everything
            .and_then(|state| state.building.as_ref())
            .map(|progress| progress.load(Ordering::Relaxed)),
    }
}

/// Reads the saved index on first use and walks the drives again when it is
/// missing or old.
pub(crate) fn ensure(cx: &mut App) {
    let state = cx.default_global::<Everything>();
    if state.index.is_some() {
        let stale = state
            .index
            .as_ref()
            .is_some_and(|index| index.built.elapsed().map_or(true, |age| age > STALE_AFTER));
        if stale && state.building.is_none() {
            rebuild(cx);
        }
        return;
    }
    if state.loading || state.building.is_some() {
        return;
    }
    state.loading = true;
    let load = cx.background_spawn(async { load(&index_file()) });
    let task = cx.spawn(async move |cx| {
        let loaded = load.await;
        cx.update(|cx| {
            let state = cx.default_global::<Everything>();
            state.loading = false;
            let stale = loaded.as_ref().map_or(true, |index| index.built.elapsed().map_or(true, |age| age > STALE_AFTER));
            if let Some(index) = loaded {
                state.index = Some(Arc::new(index));
                state.generation += 1;
            }
            if stale {
                rebuild(cx);
            }
        })
        .log_err();
    });
    cx.global_mut::<Everything>()._task = Some(task);
}

// What the walk thread reports.
enum Walked {
    // The table of the drive now being walked.
    Walking(Arc<Mutex<Segment>>),
    // Every drive finished so far.
    Drives(Vec<Arc<Segment>>),
    All(Index),
}

/// Walks the drives again now, keeping the current index until it is done.
pub(crate) fn rebuild(cx: &mut App) {
    let state = cx.default_global::<Everything>();
    if state.building.is_some() {
        return;
    }
    let progress = Arc::new(AtomicUsize::new(0));
    state.building = Some(progress.clone());
    let (sender, receiver) = async_channel::unbounded::<Walked>();
    let spawned = std::thread::Builder::new().name("nohrs-everything".into()).spawn({
        let progress = progress.clone();
        move || {
            let index = build(
                progress,
                |table| {
                    sender.send_blocking(Walked::Walking(table)).log_err();
                },
                |segments| {
                    sender.send_blocking(Walked::Drives(segments)).log_err();
                },
            );
            if let Err(error) = save(&index, &index_file()) {
                tracing::warn!("could not save the everything index: {error}");
            }
            sender.send_blocking(Walked::All(index)).log_err();
        }
    });
    if let Err(error) = spawned {
        tracing::error!("could not start the everything walk: {error}");
        cx.global_mut::<Everything>().building = None;
        return;
    }
    let task = cx.spawn(async move |cx| {
        // Wakes the panes a few times a second so the count moves while the
        // walk runs; the walk itself never waits on the UI.
        let mut ticks = 0u32;
        loop {
            cx.background_executor().timer(Duration::from_millis(400)).await;
            ticks += 1;
            match receiver.try_recv() {
                Ok(Walked::Walking(table)) => {
                    if cx.update(|cx| cx.global_mut::<Everything>().walking = Some(table)).is_err() {
                        return;
                    }
                }
                // A drive is done: searchable now, beside what the previous
                // index knew of the drives still being walked.
                Ok(Walked::Drives(done)) => {
                    let updated = cx.update(|cx| {
                        let state = cx.global_mut::<Everything>();
                        let mut segments = done.clone();
                        if let Some(old) = &state.index {
                            segments.extend(
                                old.segments
                                    .iter()
                                    .filter(|segment| !done.iter().any(|new| new.root() == segment.root()))
                                    .cloned(),
                            );
                        }
                        let built = state.index.as_ref().map_or_else(SystemTime::now, |old| old.built);
                        state.index = Some(Arc::new(Index { segments, built }));
                        state.walking = None;
                        state.generation += 1;
                    });
                    if updated.is_err() {
                        return;
                    }
                }
                Ok(Walked::All(index)) => {
                    cx.update(|cx| {
                        let state = cx.global_mut::<Everything>();
                        state.index = Some(Arc::new(index));
                        state.building = None;
                        state.walking = None;
                        state.generation += 1;
                    })
                    .log_err();
                    return;
                }
                Err(async_channel::TryRecvError::Closed) => {
                    cx.update(|cx| {
                        let state = cx.global_mut::<Everything>();
                        state.building = None;
                        state.walking = None;
                    })
                    .log_err();
                    return;
                }
                // Still walking: touching the global repaints the count. On
                // a first walk (no older table of the drive to answer from),
                // every few seconds a search also reruns, to take in what the
                // walk found since.
                Err(async_channel::TryRecvError::Empty) => {
                    let touched = cx.update(|cx| {
                        let state = cx.global_mut::<Everything>();
                        if ticks % 12 == 0 && state.walking_is_new() {
                            state.generation += 1;
                        }
                    });
                    if touched.is_err() {
                        return;
                    }
                }
            }
        }
    });
    cx.global_mut::<Everything>()._task = Some(task);
}

// ---------------------------------------------------------------------------
// The pane

/// The pane's side of the mode.
pub(crate) struct EverythingMode {
    // The query and index generation the hits on screen are for.
    last: Option<(String, u64)>,
    cancel: Arc<AtomicBool>,
    _subscription: Subscription,
}

impl ExplorerPane {
    /// Whether the bar searches the whole computer.
    pub(crate) fn everything_on(&self) -> bool {
        self.everything.is_some()
    }

    /// Turns the mode on or off. On, it leaves subfolder search and starts
    /// reading the index; off, the folder comes back.
    pub(crate) fn set_everything(&mut self, on: bool, cx: &mut Context<Self>) {
        if on == self.everything_on() {
            return;
        }
        if on {
            self.quick_recursive = false;
            self.name_filter.clear();
            self.end_subfolder_search();
            ensure(cx);
            // The global changes when the index arrives or the walk counts
            // more names: rerun or repaint.
            let subscription = cx.observe_global::<Everything>(|_pane, cx| cx.notify());
            self.everything = Some(EverythingMode {
                last: None,
                cancel: Arc::new(AtomicBool::new(false)),
                _subscription: subscription,
            });
        } else {
            if let Some(mode) = self.everything.take() {
                mode.cancel.store(true, Ordering::Relaxed);
            }
            self.end_subfolder_search();
        }
        cx.notify();
    }

    /// Runs `query` against the index, unless the hits on screen already
    /// answer it. Called each render with what the bar holds.
    pub(crate) fn search_everything(&mut self, query: String, cx: &mut Context<Self>) {
        let Some(mode) = self.everything.as_mut() else {
            return;
        };
        let state = cx.try_global::<Everything>();
        let generation = state.map_or(0, |state| state.generation);
        let index = state.and_then(|state| state.index.clone());
        let walking = state.filter(|state| state.walking_is_new()).and_then(|state| state.walking.clone());
        let key = (query.clone(), generation);
        if mode.last.as_ref() == Some(&key) {
            return;
        }
        if query.trim().is_empty() {
            mode.last = Some(key);
            mode.cancel.store(true, Ordering::Relaxed);
            self.end_subfolder_search();
            return;
        }
        if index.is_none() && walking.is_none() {
            // Nothing to search yet; the walk reruns this.
            return;
        }
        let Some(parsed) = Query::parse(&query) else {
            return;
        };
        mode.last = Some(key);
        mode.cancel.store(true, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        mode.cancel = cancel.clone();

        let (sender, receiver) = async_channel::unbounded::<Vec<FileEntryDto>>();
        let spawned = std::thread::Builder::new().name("nohrs-everything-search".into()).spawn(move || {
            let started = Instant::now();
            let targets = gather(index.as_deref(), walking.as_deref(), &parsed, &cancel);
            tracing::debug!(hits = targets.len(), elapsed_ms = started.elapsed().as_millis() as u64, "everything search");
            let mut batch = Vec::new();
            let mut limit = FIRST_BATCH;
            for (name, path, dir) in targets {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                // Gone since the walk: not shown.
                let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                batch.push(FileEntryDto {
                    name,
                    path,
                    kind: if dir { "dir" } else { "file" }.to_string(),
                    size: if dir { 0 } else { metadata.len() },
                    modified: metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
                        .map_or(0, |since| since.as_secs()),
                });
                if batch.len() >= limit {
                    if sender.send_blocking(std::mem::take(&mut batch)).is_err() {
                        return;
                    }
                    limit = BATCH;
                }
            }
            if !batch.is_empty() {
                sender.send_blocking(batch).log_err();
            }
        });
        if let Err(error) = spawned {
            tracing::error!("could not start the everything search: {error}");
            return;
        }
        self.clear_selection();
        self.hit_folders.clear();
        self.filtered_entries.clear();
        self.search_results = Some(Vec::new());
        self.update_item_sizes();
        let task = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            while let Ok(batch) = receiver.recv().await {
                if this.update(cx, |pane, cx| {
                    pane.add_search_hits(batch);
                    cx.notify();
                })
                .is_err()
                {
                    return;
                }
            }
            this.update(cx, |pane, cx| {
                if let Some(search) = pane.deep_search.as_mut() {
                    search.running = false;
                }
                cx.notify();
            })
            .log_err();
        });
        self.deep_search = Some(DeepSearch::new(query, task));
        cx.notify();
    }

    /// The bar's status while the index is being read or walked.
    pub(crate) fn everything_status(&self, cx: &App) -> Option<String> {
        self.everything.as_ref()?;
        let state = cx.try_global::<Everything>()?;
        match (&state.index, &state.building) {
            (None, Some(progress)) => Some(nohrs_core::tf!(
                "Indexando o computador… {n} itens",
                n = progress.load(Ordering::Relaxed)
            )),
            (None, None) => Some(nohrs_core::t!("Lendo o índice…").to_string()),
            _ => None,
        }
    }
}

// The best hits of the index and of the drive being walked, merged and
// ranked, as (name, path, is a folder). The walk's table is read under its
// lock and let go before any disk is touched.
fn gather(
    index: Option<&Index>,
    walking: Option<&Mutex<Segment>>,
    query: &Query,
    cancel: &AtomicBool,
) -> Vec<(String, String, bool)> {
    // The walking drive's hits carry `usize::MAX` as their segment.
    let mut hits: Vec<(u8, u16, usize, u32)> = Vec::new();
    if let Some(index) = index {
        for (segment_index, segment) in index.segments.iter().enumerate() {
            hits.extend(
                segment
                    .search(query, cancel)
                    .into_iter()
                    .map(|(rank, length, position)| (rank, length, segment_index, position)),
            );
        }
    }
    let table = walking.map(lock);
    if let Some(table) = &table {
        hits.extend(
            table
                .search(query, cancel)
                .into_iter()
                .map(|(rank, length, position)| (rank, length, usize::MAX, position)),
        );
    }
    hits.sort_unstable();
    hits.truncate(MAX_RESULTS);
    hits.into_iter()
        .filter_map(|(_, _, segment_index, position)| {
            let segment: &Segment = if segment_index == usize::MAX {
                table.as_deref()?
            } else {
                index?.segments.get(segment_index)?
            };
            let entry = segment.entries.get(position as usize)?;
            Some((segment.name(entry).to_string(), segment.path(position), entry.dir))
        })
        .collect()
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod platform {
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW};
    use windows::core::HSTRING;

    // DRIVE_FIXED; the constant's type moved between windows-rs versions.
    const DRIVE_FIXED: u32 = 3;

    /// `C:\`, `D:\`… for every fixed drive but cloud drives (Google Drive
    /// mounts one), whose every folder read is a trip to the network.
    pub fn fixed_drives() -> Vec<String> {
        let mask = unsafe { GetLogicalDrives() };
        (0..26u8)
            .filter(|bit| mask & (1 << bit) != 0)
            .map(|bit| format!("{}:\\", char::from(b'A' + bit)))
            .filter(|root| {
                let wide = HSTRING::from(root.as_str());
                if unsafe { GetDriveTypeW(&wide) } != DRIVE_FIXED {
                    return false;
                }
                let mut label = [0u16; 261];
                let named = unsafe { GetVolumeInformationW(&wide, Some(&mut label), None, None, None, None) }.is_ok();
                let label = String::from_utf16_lossy(&label);
                !(named && label.trim_end_matches('\0').to_lowercase().contains("google drive"))
            })
            .collect()
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    pub fn fixed_drives() -> Vec<String> {
        vec!["/".to_string()]
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn sample() -> Index {
        let mut other = Segment::empty();
        let d = other.push(NO_PARENT, "D:", true);
        other.push(d, "kickstarter.txt", false);
        let mut builder = Segment::empty();
        let drive = builder.push(NO_PARENT, "C:", true);
        let music = builder.push(drive, "Música", true);
        builder.push(music, "Kick 01.WAV", false);
        builder.push(music, "kick", false);
        let projects = builder.push(drive, "Projetos", true);
        builder.push(projects, "kick_final.mp3", false);
        Index { segments: vec![Arc::new(builder), Arc::new(other)], built: SystemTime::now() }
    }

    fn names(index: &Index, query: &str) -> Vec<String> {
        let query = Query::parse(query).unwrap();
        gather(Some(index), None, &query, &AtomicBool::new(false))
            .into_iter()
            .map(|(_, path, _)| path)
            .collect()
    }

    #[test]
    fn exact_names_come_first_and_case_does_not_matter() {
        let index = sample();
        assert_eq!(
            names(&index, "KICK"),
            ["C:\\Música\\kick", "C:\\Música\\Kick 01.WAV", "C:\\Projetos\\kick_final.mp3", "D:\\kickstarter.txt"]
        );
    }

    #[test]
    fn every_term_must_match_and_slashes_match_the_path() {
        let index = sample();
        assert_eq!(names(&index, "kick projetos\\"), ["C:\\Projetos\\kick_final.mp3"]);
        assert_eq!(names(&index, "*.wav"), ["C:\\Música\\Kick 01.WAV"]);
        assert_eq!(names(&index, "música"), ["C:\\Música"]);
    }

    #[test]
    fn the_saved_table_reads_back_the_same() {
        let index = sample();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("everything.idx");
        save(&index, &path).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.len(), index.len());
        assert_eq!(loaded.segments.len(), 2);
        assert_eq!(names(&loaded, "kick"), names(&index, "kick"));
        // A cut file is refused instead of read wrong.
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, &bytes[..bytes.len() - 3]).unwrap();
        assert!(load(&path).is_none());
    }

    #[test]
    fn a_drive_being_walked_is_searched_beside_the_finished_ones() {
        let index = sample();
        let mut walking = Segment::empty();
        let e = walking.push(NO_PARENT, "E:", true);
        walking.push(e, "Kick.wav", false);
        let walking = Mutex::new(walking);
        let query = Query::parse("kick").unwrap();
        let found = gather(Some(&index), Some(&walking), &query, &AtomicBool::new(false));
        let paths: Vec<&str> = found.iter().map(|(_, path, _)| path.as_str()).collect();
        assert_eq!(paths.first(), Some(&"C:\\Música\\kick"));
        assert!(paths.contains(&"E:\\Kick.wav"));
        assert_eq!(paths.len(), 5);
    }

    #[test]
    fn the_root_walks_users_first_and_windows_last() {
        let mut children = vec![
            (PathBuf::from("C:\\Users"), 1),
            (PathBuf::from("C:\\Windows"), 2),
            (PathBuf::from("C:\\Program Files"), 3),
            (PathBuf::from("C:\\Projetos"), 4),
        ];
        children.sort_by_key(|(path, _)| walk_priority(path));
        // Taken from the end: Windows last of all, Users first.
        let order: Vec<u32> = children.iter().rev().map(|(_, id)| *id).collect();
        assert_eq!(order, [1, 4, 3, 2]);
    }

    #[test]
    fn wildcards_cover_the_whole_name() {
        let pattern: Vec<char> = "k?ck*.wav".chars().collect();
        assert!(wildcard(&pattern, "kick 01.wav"));
        assert!(!wildcard(&pattern, "kick 01.wav.bak"));
        assert!(wildcard(&"*".chars().collect::<Vec<_>>(), ""));
    }
}
