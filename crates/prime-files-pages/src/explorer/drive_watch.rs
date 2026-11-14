//! The watcher behind the index's fresh names (`watch_roots`): every name
//! created or renamed under a root, read straight from
//! `ReadDirectoryChangesW`.
//!
//! Until 13/11/2026 this was the `notify` crate, and two of its choices cost
//! names with no way to tell:
//! - a change buffer of 16 KB, and an overflow that arrives as success with
//!   nothing in it, so the lost names were never reported;
//! - `ERROR_NOTIFY_ENUM_DIR` (the same overflow, as an error) taken as
//!   "unknown error" and answered with an unwatch of the whole drive, until
//!   the app was opened again.
//!
//! Here the buffer is 64 KB (the most a network share takes; local drives
//! take more, but the kernel holds that much per watched root), an overflow
//! in either form is reported (`on_lost`) so the index can be walked again,
//! and any other error reopens the root after a pause instead of giving up.

use std::path::PathBuf;

/// Bytes Windows may queue between two reads of one root.
pub(super) const BUFFER: usize = 64 * 1024;

/// What a root reports.
pub(super) enum Change {
    /// A name that appeared: created, or the new name of a rename.
    Appeared(PathBuf),
    /// Names were dropped by Windows (the buffer filled before it was read):
    /// what is under this root may be missing until the next walk.
    Lost(PathBuf),
}

/// Watches roots until dropped.
pub(super) struct DriveWatch {
    #[cfg(target_os = "windows")]
    inner: platform::Watch,
}

impl DriveWatch {
    /// Watches every root, with `buffer` bytes of room for each between
    /// reads, and hands `report` what it sees, on a thread per root.
    pub(super) fn start(
        roots: &[PathBuf],
        buffer: usize,
        report: impl Fn(Change) + Send + Sync + 'static,
    ) -> Option<Self> {
        #[cfg(target_os = "windows")]
        {
            platform::Watch::start(roots, buffer, report).map(|inner| Self { inner })
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (roots, buffer, report);
            None
        }
    }
}

#[cfg(target_os = "windows")]
impl Drop for DriveWatch {
    fn drop(&mut self) {
        self.inner.stop();
    }
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod platform {
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Duration;

    use windows::Win32::Foundation::{
        CloseHandle, ERROR_NOTIFY_ENUM_DIR, ERROR_OPERATION_ABORTED, HANDLE, WAIT_OBJECT_0,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ACTION_ADDED, FILE_ACTION_RENAMED_NEW_NAME, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
        FILE_NOTIFY_INFORMATION, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        ReadDirectoryChangesW,
    };
    use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects, WaitForSingleObject};
    use windows::core::PCWSTR;

    use super::Change;

    /// How long a root whose read failed waits before it is opened again.
    const REOPEN_AFTER: Duration = Duration::from_secs(5);

    // A handle shared with the threads, closed once they are gone.
    struct Event(HANDLE);
    unsafe impl Send for Event {}
    unsafe impl Sync for Event {}

    pub(super) struct Watch {
        stop: Arc<Event>,
        threads: Vec<JoinHandle<()>>,
    }

    impl Watch {
        pub(super) fn start(
            roots: &[PathBuf],
            buffer: usize,
            report: impl Fn(Change) + Send + Sync + 'static,
        ) -> Option<Self> {
            // Manual reset: once set, every thread sees it.
            let stop = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.ok()?;
            let stop = Arc::new(Event(stop));
            let report: Arc<dyn Fn(Change) + Send + Sync> = Arc::new(report);
            let mut threads = Vec::new();
            for root in roots {
                let root = root.clone();
                let stop = stop.clone();
                let report = report.clone();
                let spawned = std::thread::Builder::new()
                    .name("prime-files-drive-watch".into())
                    .spawn(move || watch_root(&root, buffer, &stop, &*report));
                match spawned {
                    Ok(thread) => threads.push(thread),
                    Err(error) => tracing::warn!("cannot watch a drive for the everything index: {error}"),
                }
            }
            Some(Self { stop, threads })
        }

        pub(super) fn stop(&mut self) {
            unsafe { SetEvent(self.stop.0) }.ok();
            for thread in self.threads.drain(..) {
                thread.join().ok();
            }
            unsafe { CloseHandle(self.stop.0) }.ok();
        }
    }

    // Reads one root's changes until `stop` is set, reopening it after any
    // failure.
    fn watch_root(root: &std::path::Path, buffer: usize, stop: &Event, report: &dyn Fn(Change)) {
        // DWORD-aligned, as the records require.
        let mut room = vec![0u32; buffer.div_ceil(4).max(1)];
        let mut opened_before = false;
        loop {
            match read_until_failure(root, &mut room, stop, report, opened_before) {
                Ended::Stopped => return,
                Ended::Failed => {
                    opened_before = true;
                    // What changed while it was closed is not known.
                    report(Change::Lost(root.to_path_buf()));
                    if unsafe { WaitForSingleObject(stop.0, REOPEN_AFTER.as_millis() as u32) } == WAIT_OBJECT_0 {
                        return;
                    }
                }
            }
        }
    }

    enum Ended {
        Stopped,
        Failed,
    }

    fn read_until_failure(
        root: &std::path::Path,
        room: &mut [u32],
        stop: &Event,
        report: &dyn Fn(Change),
        reopened: bool,
    ) -> Ended {
        let wide: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
        let directory = unsafe {
            CreateFileW(
                PCWSTR(wide.as_ptr()),
                FILE_LIST_DIRECTORY.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                None,
            )
        };
        let directory = match directory {
            Ok(directory) => directory,
            Err(error) => {
                if !reopened {
                    tracing::warn!("cannot watch {} for the everything index: {error}", root.display());
                }
                return Ended::Failed;
            }
        };
        let Ok(done) = (unsafe { CreateEventW(None, true, false, PCWSTR::null()) }) else {
            unsafe { CloseHandle(directory) }.ok();
            return Ended::Failed;
        };
        let ended = loop {
            let mut overlapped = OVERLAPPED { hEvent: done, ..Default::default() };
            let bytes = (room.len() * 4) as u32;
            let started = unsafe {
                ReadDirectoryChangesW(
                    directory,
                    room.as_mut_ptr().cast(),
                    bytes,
                    true,
                    FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME,
                    None,
                    Some(&mut overlapped),
                    None,
                )
            };
            if let Err(error) = started {
                tracing::debug!("ReadDirectoryChangesW on {} failed: {error}", root.display());
                break Ended::Failed;
            }
            let waited = unsafe { WaitForMultipleObjects(&[stop.0, done], false, INFINITE) };
            if waited == WAIT_OBJECT_0 {
                // The read must be over before its buffer and OVERLAPPED go.
                unsafe { CancelIoEx(directory, Some(&overlapped)) }.ok();
                let mut ignored = 0;
                unsafe { GetOverlappedResult(directory, &overlapped, &mut ignored, true) }.ok();
                break Ended::Stopped;
            }
            let mut written = 0u32;
            match unsafe { GetOverlappedResult(directory, &overlapped, &mut written, false) } {
                Ok(()) if written == 0 => {
                    // The overflow notify never reported: success, no records.
                    report(Change::Lost(root.to_path_buf()));
                }
                Ok(()) => {
                    let used = (written as usize).min(room.len() * 4);
                    for path in appeared(root, room, used) {
                        report(Change::Appeared(path));
                    }
                }
                Err(error) if error.code() == ERROR_NOTIFY_ENUM_DIR.to_hresult() => {
                    // The same overflow as an error: the handle still works.
                    report(Change::Lost(root.to_path_buf()));
                }
                Err(error) if error.code() == ERROR_OPERATION_ABORTED.to_hresult() => break Ended::Stopped,
                Err(error) => {
                    tracing::debug!("watching {} failed: {error}", root.display());
                    break Ended::Failed;
                }
            }
            unsafe { windows::Win32::System::Threading::ResetEvent(done) }.ok();
        };
        unsafe { CloseHandle(done) }.ok();
        unsafe { CloseHandle(directory) }.ok();
        ended
    }

    // The names created and renamed into place among the records in the
    // first `used` bytes of `room`.
    fn appeared(root: &std::path::Path, room: &[u32], used: usize) -> Vec<PathBuf> {
        let bytes = unsafe { std::slice::from_raw_parts(room.as_ptr().cast::<u8>(), used) };
        let header = std::mem::offset_of!(FILE_NOTIFY_INFORMATION, FileName);
        let mut found = Vec::new();
        let mut at = 0usize;
        while at + header <= bytes.len() {
            let record = unsafe { std::ptr::read_unaligned(bytes.as_ptr().add(at).cast::<FILE_NOTIFY_INFORMATION>()) };
            let length = record.FileNameLength as usize;
            let start = at + header;
            if start + length > bytes.len() {
                break;
            }
            if record.Action == FILE_ACTION_ADDED || record.Action == FILE_ACTION_RENAMED_NEW_NAME {
                let units: Vec<u16> = bytes[start..start + length]
                    .chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                    .collect();
                found.push(root.join(std::ffi::OsString::from_wide(&units)));
            }
            if record.NextEntryOffset == 0 {
                break;
            }
            at += record.NextEntryOffset as usize;
        }
        found
    }
}
