//! Phones, cameras and media players plugged in by USB (MTP), opened here
//! instead of handed to the Explorer. Until 10/11/2026 a phone showed
//! nowhere in Prime Files (a report on X), and then only as a row that
//! opened the Explorer.
//!
//! None of it is on disk: there is no path to read with `std::fs`, only the
//! Shell's namespace. Each row's path is the item's Shell parsing name
//! (`::{20D04FE0-…}\\?\usb#…\SID-{…}\{…}`), which the Shell parses back
//! into the item, so icons, thumbnails, Properties and dragging out work
//! as for any other row. Reading, copying, deleting and renaming go
//! through the Shell too (`IFileOperation`), with its own progress and
//! conflict windows, the same ones the Explorer shows for a phone.

use std::collections::HashMap;
use std::sync::Mutex;

use prime_files_services::fs::listing::FileEntryDto;

pub(crate) use super::places::is_device;

/// One step of the way from This PC down to a device folder: the name
/// shown and the address it opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Crumb {
    pub name: String,
    pub address: String,
}

// The way down to each device folder read, by its address in lowercase:
// what the header draws and where "up" leads. Filled by the listing's
// thread when it reads the folder, so the UI never asks the Shell (a phone
// can take seconds to answer).
static TRAILS: Mutex<Option<HashMap<String, Vec<Crumb>>>> = Mutex::new(None);

fn remember_trail(address: &str, trail: Vec<Crumb>) {
    if let Ok(mut trails) = TRAILS.lock() {
        let trails = trails.get_or_insert_with(HashMap::new);
        // A session visits a few hundred folders at most; past that the
        // oldest are as good as any to forget.
        if trails.len() > 2_000 {
            trails.clear();
        }
        trails.insert(address.to_lowercase(), trail);
    }
}

/// The way from the device down to `address`, the device first, as last
/// read. `None` before the folder was read.
pub(crate) fn trail(address: &str) -> Option<Vec<Crumb>> {
    TRAILS
        .lock()
        .ok()
        .and_then(|trails| trails.as_ref().and_then(|trails| trails.get(&address.to_lowercase()).cloned()))
}

/// The name of the device folder at `address`, as the Explorer shows it.
pub(crate) fn name(address: &str) -> Option<String> {
    trail(address).and_then(|trail| trail.last().map(|crumb| crumb.name.clone()))
}

/// Where "up" leads from a device folder: the folder above it, and This PC
/// from the device itself (or from a folder not read yet).
pub(crate) fn parent(address: &str) -> String {
    trail(address)
        .filter(|trail| trail.len() > 1)
        .and_then(|trail| trail.get(trail.len() - 2).map(|crumb| crumb.address.clone()))
        .unwrap_or_else(|| super::places::THIS_PC.to_string())
}

/// What a device folder holds. Runs on the listing's own thread: the Shell
/// answers for a phone at the phone's pace.
pub(super) fn read_entries(address: &str) -> Result<Vec<FileEntryDto>, String> {
    let (entries, trail) = platform::read(address)?;
    remember_trail(address, trail);
    Ok(entries)
}

// The operations below block until the Shell is done (its progress window
// up the whole time) and want a single-threaded COM apartment, which gpui's
// pool threads are not, and a panic there aborts the app: they run through
// `ExplorerPane::run_device_op`, each on a thread of its own, as the Recycle
// Bin's do.

/// Copies (or moves) `sources` into the folder `dest`, either side a device
/// or a folder on disk, with the Shell's progress and conflict windows.
pub(crate) fn transfer(sources: Vec<String>, dest: String, moving: bool, owner: Option<i64>) -> Result<(), String> {
    with_ole_result(|| platform::transfer(&sources, &dest, moving, owner))
}

/// Copies what the clipboard holds, as the Explorer put it there (a phone's
/// files have no path, only Shell items), into `dest`.
pub(crate) fn paste_clipboard(dest: String, owner: Option<i64>) -> Result<(), String> {
    with_ole_result(|| platform::paste_clipboard(&dest, owner))
}

/// Deletes `items` from the device, after the Shell's own confirmation: a
/// phone has no Recycle Bin, so it is for good, as in the Explorer.
pub(crate) fn delete(items: Vec<String>, owner: Option<i64>) -> Result<(), String> {
    with_ole_result(|| platform::delete(&items, owner))
}

/// Renames the item at `address` to `name`.
pub(crate) fn rename(address: String, name: String, owner: Option<i64>) -> Result<(), String> {
    with_ole_result(|| platform::rename(&address, &name, owner))
}

/// Makes a folder called `name` (numbered by the Shell if taken) in `dest`.
pub(crate) fn new_folder(dest: String, name: String, owner: Option<i64>) -> Result<(), String> {
    with_ole_result(|| platform::new_folder(&dest, &name, owner))
}

impl super::ExplorerPane {
    /// Runs one of the Shell operations above on a thread of its own, then
    /// reads the folder again. The Shell's own windows tell how it went;
    /// only a refusal (a phone unplugged, locked) reaches the status bar.
    pub(crate) fn run_device_op(
        &mut self,
        op: impl FnOnce(Option<i64>) -> Result<(), String> + Send + 'static,
        owner: Option<i64>,
        cx: &mut gpui::Context<Self>,
    ) {
        // A copy from a phone into the folder on screen writes there for as
        // long as the Shell copies; the watch waits for the one reload below
        // instead of reading the folder every 800 ms (`run_fs_op_with` does
        // the same; docs/PERFORMANCE.md, pass 15).
        let quiet = self.begin_own_operation();
        let (sender, receiver) = async_channel::bounded(1);
        let spawned = std::thread::Builder::new().name("device".into()).spawn(move || {
            let _ = sender.send_blocking(op(owner));
        });
        if let Err(error) = spawned {
            self.set_status(super::types::StatusLevel::Error, prime_files_core::tf!("O aparelho recusou: {error}", error = error));
            cx.notify();
            return;
        }
        cx.spawn(async move |this: gpui::WeakEntity<Self>, cx| {
            // A thread that died without answering panicked: say so.
            let outcome = receiver.recv().await.unwrap_or_else(|_| Err(String::from("panic")));
            this.update(cx, |pane, cx| {
                // Ended just before the reload, which reads what the watch
                // heard until now.
                drop(quiet);
                pane.reload(cx);
                // After the reload, which clears the status: set before it,
                // the device's refusal was wiped before anyone saw it.
                if let Err(error) = outcome {
                    pane.set_status(super::types::StatusLevel::Error, prime_files_core::tf!("O aparelho recusou: {error}", error = error));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Copy (or Cut) of items on a phone: the Shell's data on the clipboard.
    /// A phone's files cannot be moved off it by a paste here, so Cut copies
    /// and says so.
    pub(crate) fn copy_device_items(
        &mut self,
        mode: super::file_ops::ClipMode,
        paths: &[String],
        cx: &mut gpui::Context<Self>,
    ) {
        use super::types::StatusLevel;
        use prime_files_core::{i18n, k, t, tf};
        match copy_to_clipboard(paths) {
            Ok(()) => {
                // What Prime Files copied before is no longer what a paste
                // takes.
                super::file_ops::ExplorerClipboard::write(None, cx);
                let message = match mode {
                    super::file_ops::ClipMode::Copy => {
                        i18n::plural(paths.len(), k!("{n} item copiado"), k!("{n} itens copiados"))
                    }
                    super::file_ops::ClipMode::Cut => t!("No aparelho, Recortar copia: apague depois de colar.").to_string(),
                };
                self.set_status(StatusLevel::Info, message);
            }
            Err(error) => self.set_status(StatusLevel::Error, tf!("O aparelho recusou: {error}", error = error)),
        }
        cx.notify();
    }

    /// A paste the Shell does instead of Prime Files: into a phone, or of
    /// a phone's files the Explorer copied (Shell items with no path).
    /// Whether it took the paste.
    pub(crate) fn paste_through_shell(&mut self, dest: String, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> bool {
        let into_device = is_device(&dest);
        let clipboard = self.read_clipboard(cx);
        if !into_device && (clipboard.is_some() || !clipboard_has_items()) {
            return false;
        }
        let owner = crate::ipc::window_hwnd(window);
        match clipboard {
            Some((mode, sources)) => {
                let moving = mode == super::file_ops::ClipMode::Cut;
                let sources: Vec<String> = sources.iter().map(|path| path.to_string_lossy().into_owned()).collect();
                self.run_device_op(move |owner| transfer(sources, dest, moving, owner), owner, cx);
            }
            None => self.run_device_op(move |owner| paste_clipboard(dest, owner), owner, cx),
        }
        true
    }
}

/// Puts `items` on the clipboard as the Explorer's Copy does, so a paste
/// here, in the Explorer or on the Desktop copies them off the device.
/// On the UI thread: the clipboard keeps asking the thread that put the
/// data there for it, and that thread has to stay alive and pumping.
pub(crate) fn copy_to_clipboard(items: &[String]) -> Result<(), String> {
    platform::copy_to_clipboard(items).map_err(|error| error.to_string())
}

/// Whether the clipboard holds Shell items (a phone's files copied in the
/// Explorer) that a paste can copy here.
pub(crate) fn clipboard_has_items() -> bool {
    platform::clipboard_has_items()
}

#[cfg(windows)]
mod windows_result {
    pub type Result<T> = windows::core::Result<T>;
}

#[cfg(not(windows))]
mod windows_result {
    pub struct Error;
    impl Error {
        pub fn message(&self) -> String {
            String::from("unsupported")
        }
    }
    pub type Result<T> = std::result::Result<T, Error>;
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    //! The Shell's namespace and file operations have no safe binding.

    use std::ffi::c_void;

    use prime_files_services::fs::listing::FileEntryDto;
    use windows::Win32::Foundation::{FILETIME, HWND};
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, IDataObject};
    use windows::Win32::System::DataExchange::{IsClipboardFormatAvailable, RegisterClipboardFormatW};
    use windows::Win32::System::Ole::{OleGetClipboard, OleInitialize, OleSetClipboard, OleUninitialize};
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::SystemServices::{SFGAO_FOLDER, SFGAO_STREAM};
    use windows::Win32::UI::Shell::PropertiesSystem::PSGetPropertyKeyFromName;
    use windows::Win32::UI::Shell::{
        BHID_DataObject, BHID_EnumItems, FOF_NOCONFIRMMKDIR, FileOperation, IEnumShellItems, IFileOperation,
        IShellItem, IShellItem2, IShellItemArray, ILFree, SHCreateItemFromParsingName,
        SHCreateShellItemArrayFromDataObject, SHCreateShellItemArrayFromIDLists, SHParseDisplayName, SIGDN,
        SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY,
    };
    use windows::core::{HSTRING, Interface, PCWSTR, w};

    use super::Crumb;
    use crate::explorer::places::THIS_PC;

    pub fn with_ole<T>(work: impl FnOnce() -> windows::core::Result<T>) -> windows::core::Result<T> {
        let initialized = unsafe { OleInitialize(None) }.is_ok();
        let result = work();
        if initialized {
            unsafe { OleUninitialize() };
        }
        result
    }

    fn item(address: &str) -> windows::core::Result<IShellItem> {
        unsafe { SHCreateItemFromParsingName(&HSTRING::from(address), None) }
    }

    fn display_name(item: &IShellItem, kind: SIGDN) -> Option<String> {
        let name = unsafe { item.GetDisplayName(kind) }.ok()?;
        let text = unsafe { name.to_string() }.ok();
        unsafe { CoTaskMemFree(Some(name.0 as *const _)) };
        text.filter(|text| !text.is_empty())
    }

    fn key(name: PCWSTR) -> Option<PROPERTYKEY> {
        let mut key = PROPERTYKEY::default();
        unsafe { PSGetPropertyKeyFromName(name, &mut key) }.ok()?;
        Some(key)
    }

    fn string(item: &IShellItem2, key: Option<&PROPERTYKEY>) -> Option<String> {
        let value = unsafe { item.GetString(key?) }.ok()?;
        let text = unsafe { value.to_string() }.ok();
        unsafe { CoTaskMemFree(Some(value.0 as *const _)) };
        text.filter(|text| !text.is_empty())
    }

    // FILETIME counts 100 ns since 1601; the listing wants Unix seconds.
    fn unix_seconds(time: FILETIME) -> u64 {
        let ticks = (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
        (ticks / 10_000_000).saturating_sub(11_644_473_600)
    }

    pub fn read(address: &str) -> Result<(Vec<FileEntryDto>, Vec<Crumb>), String> {
        super::with_ole_result(|| {
            let folder = item(address)?;
            let trail = trail_of(&folder);
            let enumerator: IEnumShellItems = unsafe { folder.BindToHandler(None, &BHID_EnumItems) }?;
            let file_name = key(w!("System.FileName"));
            let size = key(w!("System.Size"));
            let modified = key(w!("System.DateModified"));
            let mut entries = Vec::new();
            loop {
                let mut batch = [None];
                let mut fetched = 0;
                if unsafe { enumerator.Next(&mut batch, Some(&mut fetched)) }.is_err() || fetched == 0 {
                    break;
                }
                let Some(child) = batch.into_iter().flatten().next() else {
                    break;
                };
                let Some(path) = display_name(&child, SIGDN_DESKTOPABSOLUTEPARSING) else {
                    continue;
                };
                let attributes = unsafe { child.GetAttributes(SFGAO_FOLDER | SFGAO_STREAM) }.unwrap_or_default();
                // A folder that is also a stream is a file the Shell can
                // browse into (a .zip): a file here, as on disk.
                let is_dir = attributes.contains(SFGAO_FOLDER) && !attributes.contains(SFGAO_STREAM);
                let rich = child.cast::<IShellItem2>().ok();
                // The file name with its extension: the display name drops
                // it when the Explorer hides extensions.
                let name = rich
                    .as_ref()
                    .and_then(|rich| string(rich, file_name.as_ref()))
                    .or_else(|| display_name(&child, SIGDN_NORMALDISPLAY))
                    .unwrap_or_else(|| path.clone());
                let size = if is_dir {
                    0
                } else {
                    rich.as_ref()
                        .zip(size.as_ref())
                        .and_then(|(rich, key)| unsafe { rich.GetUInt64(key) }.ok())
                        .unwrap_or(0)
                };
                let modified = rich
                    .as_ref()
                    .zip(modified.as_ref())
                    .and_then(|(rich, key)| unsafe { rich.GetFileTime(key) }.ok())
                    .map_or(0, unix_seconds);
                entries.push(FileEntryDto {
                    name,
                    path,
                    kind: if is_dir { "dir" } else { "file" }.to_string(),
                    size,
                    modified,
                    attributes: 0,
                });
            }
            Ok((entries, trail))
        })
    }

    // The folders from the device down to `folder`, read by walking up to
    // This PC.
    fn trail_of(folder: &IShellItem) -> Vec<Crumb> {
        let mut trail = Vec::new();
        let mut current = Some(folder.clone());
        while let Some(here) = current.take() {
            let Some(address) = display_name(&here, SIGDN_DESKTOPABSOLUTEPARSING) else {
                break;
            };
            if address.eq_ignore_ascii_case(THIS_PC) || !super::is_device(&address) || trail.len() > 64 {
                break;
            }
            let name = display_name(&here, SIGDN_NORMALDISPLAY).unwrap_or_else(|| address.clone());
            trail.push(Crumb { name, address });
            current = unsafe { here.GetParent() }.ok();
        }
        trail.reverse();
        trail
    }

    // The items at `addresses` as one Shell array, as a selection in the
    // Explorer is.
    fn items(addresses: &[String]) -> windows::core::Result<IShellItemArray> {
        let mut pidls: Vec<*mut ITEMIDLIST> = Vec::with_capacity(addresses.len());
        let result = (|| {
            for address in addresses {
                let mut pidl = std::ptr::null_mut();
                unsafe { SHParseDisplayName(&HSTRING::from(address.as_str()), None, &mut pidl, 0, None)? };
                pidls.push(pidl);
            }
            let list: Vec<*const ITEMIDLIST> = pidls.iter().map(|pidl| pidl.cast_const()).collect();
            unsafe { SHCreateShellItemArrayFromIDLists(&list) }
        })();
        for pidl in pidls {
            unsafe { ILFree(Some(pidl.cast_const())) };
        }
        result
    }

    fn operation(owner: Option<i64>) -> windows::core::Result<IFileOperation> {
        let operation: IFileOperation = unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL) }?;
        if let Some(owner) = owner {
            unsafe { operation.SetOwnerWindow(HWND(owner as *mut c_void)) }?;
        }
        Ok(operation)
    }

    // Runs what was queued. Cancelling in the Shell's window is no error:
    // the window already said what happened.
    fn perform(operation: &IFileOperation) -> windows::core::Result<()> {
        match unsafe { operation.PerformOperations() } {
            Err(error) if error.code() == windows::Win32::Foundation::E_ABORT => Ok(()),
            other => other,
        }
    }

    pub fn transfer(sources: &[String], dest: &str, moving: bool, owner: Option<i64>) -> windows::core::Result<()> {
        let operation = operation(owner)?;
        unsafe { operation.SetOperationFlags(FOF_NOCONFIRMMKDIR) }?;
        let sources = items(sources)?;
        let dest = item(dest)?;
        if moving {
            unsafe { operation.MoveItems(&sources, &dest) }?;
        } else {
            unsafe { operation.CopyItems(&sources, &dest) }?;
        }
        perform(&operation)
    }

    pub fn paste_clipboard(dest: &str, owner: Option<i64>) -> windows::core::Result<()> {
        let data = unsafe { OleGetClipboard() }?;
        let sources: IShellItemArray = unsafe { SHCreateShellItemArrayFromDataObject(&data) }?;
        let operation = operation(owner)?;
        unsafe { operation.SetOperationFlags(FOF_NOCONFIRMMKDIR) }?;
        unsafe { operation.CopyItems(&sources, &item(dest)?) }?;
        perform(&operation)
    }

    pub fn delete(addresses: &[String], owner: Option<i64>) -> windows::core::Result<()> {
        let operation = operation(owner)?;
        unsafe { operation.DeleteItems(&items(addresses)?) }?;
        perform(&operation)
    }

    pub fn rename(address: &str, name: &str, owner: Option<i64>) -> windows::core::Result<()> {
        let operation = operation(owner)?;
        unsafe { operation.RenameItem(&item(address)?, &HSTRING::from(name), None) }?;
        perform(&operation)
    }

    pub fn new_folder(dest: &str, name: &str, owner: Option<i64>) -> windows::core::Result<()> {
        use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
        let operation = operation(owner)?;
        unsafe {
            operation.NewItem(&item(dest)?, FILE_ATTRIBUTE_DIRECTORY.0, &HSTRING::from(name), PCWSTR::null(), None)
        }?;
        perform(&operation)
    }

    pub fn copy_to_clipboard(addresses: &[String]) -> windows::core::Result<()> {
        // Already done by gpui on this thread; a second call only counts.
        let initialized = unsafe { OleInitialize(None) }.is_ok();
        let result = (|| {
            let data: IDataObject = unsafe { items(addresses)?.BindToHandler(None, &BHID_DataObject) }?;
            unsafe { OleSetClipboard(&data) }
        })();
        // Uninitializing here would take the clipboard's data with it; the
        // count stays one higher for as long as the app runs, which is what
        // keeping the data there needs anyway.
        let _ = initialized;
        result
    }

    pub fn clipboard_has_items() -> bool {
        let format = unsafe { RegisterClipboardFormatW(w!("Shell IDList Array")) };
        format != 0 && unsafe { IsClipboardFormatAvailable(format) }.is_ok()
    }

    /// Opens a device file with its program, as the Explorer's double click:
    /// the Shell copies it to a temporary folder first.
    #[cfg_attr(test, allow(dead_code))]
    pub fn open(address: &str) {
        use windows::Win32::UI::Shell::{SEE_MASK_INVOKEIDLIST, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW};
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let address = address.to_owned();
        let spawned = std::thread::Builder::new().name("device-open".into()).spawn(move || {
            let _ = with_ole(|| {
                let mut pidl = std::ptr::null_mut();
                unsafe { SHParseDisplayName(&HSTRING::from(address.as_str()), None, &mut pidl, 0, None)? };
                let mut info = SHELLEXECUTEINFOW {
                    cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
                    fMask: SEE_MASK_INVOKEIDLIST | SEE_MASK_NOASYNC,
                    lpIDList: pidl.cast(),
                    nShow: SW_SHOWNORMAL.0,
                    ..Default::default()
                };
                let opened = unsafe { ShellExecuteExW(&mut info) };
                unsafe { ILFree(Some(pidl.cast_const())) };
                if let Err(error) = &opened {
                    tracing::warn!("could not open {address}: {error}");
                }
                opened
            });
        });
        if let Err(error) = spawned {
            tracing::error!("could not start the thread that opens device files: {error}");
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use prime_files_services::fs::listing::FileEntryDto;

    use super::Crumb;
    use super::windows_result::{Error, Result};

    pub fn with_ole<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
        work()
    }
    pub fn read(_address: &str) -> std::result::Result<(Vec<FileEntryDto>, Vec<Crumb>), String> {
        Err(String::from("unsupported"))
    }
    pub fn transfer(_: &[String], _: &str, _: bool, _: Option<i64>) -> Result<()> {
        Err(Error)
    }
    pub fn paste_clipboard(_: &str, _: Option<i64>) -> Result<()> {
        Err(Error)
    }
    pub fn delete(_: &[String], _: Option<i64>) -> Result<()> {
        Err(Error)
    }
    pub fn rename(_: &str, _: &str, _: Option<i64>) -> Result<()> {
        Err(Error)
    }
    pub fn new_folder(_: &str, _: &str, _: Option<i64>) -> Result<()> {
        Err(Error)
    }
    pub fn copy_to_clipboard(_: &[String]) -> Result<()> {
        Err(Error)
    }
    pub fn clipboard_has_items() -> bool {
        false
    }
    pub fn open(_address: &str) {}
}

/// Opens a device file with its program. Tests record the request instead
/// (`open_with_default_app`), so nothing calls this there.
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn open(address: &str) {
    platform::open(address);
}

// `read` on the listing's thread, inside OLE like the operations.
fn with_ole_result<T>(work: impl FnOnce() -> windows_result::Result<T>) -> Result<T, String> {
    platform::with_ole(work).map_err(|error| error.message())
}

#[cfg(test)]
mod tests {
    use super::{Crumb, parent, remember_trail};
    use crate::explorer::places::THIS_PC;

    // "Up" walks the trail the listing read, and leaves the device for This
    // PC; a folder never read goes to This PC rather than nowhere.
    #[test]
    fn up_from_a_device_folder_follows_the_trail() {
        let device = format!(r"{THIS_PC}\\?\usb#vid_test#{{6ac27878}}");
        let storage = format!(r"{device}\SID-{{10001}}");
        let dcim = format!(r"{storage}\{{0001}}");
        let crumb = |name: &str, address: &str| Crumb { name: name.into(), address: address.into() };
        remember_trail(&dcim, vec![crumb("Galaxy", &device), crumb("Interno", &storage), crumb("DCIM", &dcim)]);
        remember_trail(&device, vec![crumb("Galaxy", &device)]);
        assert_eq!(parent(&dcim), storage);
        assert_eq!(parent(&dcim.to_uppercase()), storage);
        assert_eq!(parent(&device), THIS_PC);
        assert_eq!(parent(&format!(r"{device}\nunca-lida")), THIS_PC);
        assert_eq!(super::name(&dcim).as_deref(), Some("DCIM"));
    }

    // The Shell reading of a folder with no path, on this machine's Control
    // Panel (no phone here): names, folders and files come back, and no
    // trail, since it is not under This PC. With a phone plugged in, set
    // PRIME_FILES_DEVICE to its address to read it instead. Run by hand
    // with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn reads_a_shell_folder_with_no_path() {
        let address = std::env::var("PRIME_FILES_DEVICE")
            .unwrap_or_else(|_| String::from("::{26EE0668-A00A-44D7-9371-BEB064C98683}"));
        let (entries, trail) = std::thread::spawn(move || super::platform::read(&address).unwrap()).join().unwrap();
        for entry in &entries {
            println!("{} | {} | {} | {} | {}", entry.kind, entry.name, entry.size, entry.modified, entry.path);
        }
        println!("trail: {trail:?}");
        assert!(!entries.is_empty());
    }
}
