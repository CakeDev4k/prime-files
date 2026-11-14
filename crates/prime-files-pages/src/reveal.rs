//! "Show in folder" from other programs, answered by Prime Files.
//!
//! Premiere's "Reveal in Explorer", a browser's "Show in folder" and the Start
//! menu's "Open file location" start Explorer itself (`explorer /select,…` or
//! `SHOpenFolderAndSelectItems`), never the folder verb that makes Prime Files the
//! default, so they kept opening Explorer. explorer.exe cannot be replaced, so
//! Prime Files watches for an Explorer window to appear instead. A reveal has its
//! file selected by the time the folder's items are listed; a folder opened by
//! hand has nothing selected then, and nobody clicks that fast. Such a window
//! is closed and the file shown here.
//!
//! Active whenever Prime Files runs, default folder handler or not: the owner asked
//! for Premiere's reveal without making Prime Files the default.

use gpui::{Context, WeakEntity, Window};

use crate::root::RootView;

static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Turns the takeover on or off (`explorer.reveal_from_other_apps`). Off,
/// the hook keeps running but leaves every Explorer window alone.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

fn enabled() -> bool {
    ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

// Until when Explorer windows are left alone because Prime Files opened them
// itself ("Open in File Explorer", a place only the Explorer shows).
static LEAVE_EXPLORER_UNTIL: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Lets the next Explorer window (within a few seconds) stay the
/// Explorer's: called right before Prime Files starts explorer.exe itself.
pub fn leave_next_explorer_window() {
    if let Ok(mut until) = LEAVE_EXPLORER_UNTIL.lock() {
        *until = Some(std::time::Instant::now() + std::time::Duration::from_secs(8));
    }
}

// Whether a window opened on a plain folder is taken too, not only reveals:
// with Prime Files as the default explorer, folders the Explorer opens on
// its own (its taskbar Jump List, issue #10) open here instead.
fn takes_folders() -> bool {
    let left_alone = LEAVE_EXPLORER_UNTIL
        .lock()
        .ok()
        .and_then(|until| *until)
        .is_some_and(|until| std::time::Instant::now() < until);
    !left_alone && crate::default_explorer::is_default()
}

/// Starts watching for Explorer windows opened to reveal a file.
pub fn watch(_window: &mut Window, cx: &mut Context<RootView>) {
    let (sender, receiver) = async_channel::unbounded::<String>();
    if !platform::start_hook(sender) {
        return;
    }
    // So the next time the app is closed, a folder from the Explorer's Jump
    // List still opens here (issue #60). Off the UI thread: it asks `reg`.
    std::thread::Builder::new()
        .name("prime-files-ensure-watch".into())
        .spawn(crate::default_explorer::ensure_resident_watch)
        .ok();
    // Shown in the window used last (`crate::windows`).
    cx.spawn(async move |_this: WeakEntity<RootView>, cx: &mut gpui::AsyncApp| {
        while let Ok(path) = receiver.recv().await {
            if crate::windows::with_target(cx, |root, window, cx| root.reveal_from_elsewhere(&path, window, cx)).is_none() {
                tracing::debug!("no explorer window to show {path} in");
            }
        }
    })
    .detach();
}

impl RootView {
    // The file another program asked Explorer to show, shown here instead.
    fn reveal_from_elsewhere(&mut self, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((folder, select)) = crate::ipc::folder_and_selection(std::path::Path::new(path)) else {
            return;
        };
        self.show_explorer(cx);
        self.explorer_entity()
            .update(cx, |page, cx| page.open_path(folder, select, window, cx));
        window.activate_window();
    }
}

/// The resident watch (`prime-files --watch-explorer`): the same hook with
/// no window of the app, for while Prime Files is closed.
///
/// With Prime Files the default explorer, a folder from the Explorer
/// taskbar icon's Jump List opens in an Explorer window, not through the
/// folder verb (issue #60). While the app runs its hook takes that window
/// (issue #10); with the app closed nothing did, and the folder stayed in
/// the Explorer. This process is started at sign-in and by the app, keeps
/// only the hook (a few MB), and hands each window it takes to
/// `prime-files <path>`, which starts the app. While the app runs it steps
/// aside: the app's own hook answers.
///
/// It ends when Prime Files stops being the default explorer, when Windows
/// or the installer asks its hidden window to close, or when
/// `--stop-watching` signals it.
pub fn run_resident() {
    platform::run_resident();
}

/// Asks a running resident watch to end (the uninstaller, before removing
/// the program).
pub fn stop_resident() {
    platform::stop_resident();
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    //! The hook and the reading of Explorer's selection. `unsafe` is confined
    //! here: none of these interfaces has a safe binding.

    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
        CoUninitialize, IServiceProvider,
    };
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
    use windows::Win32::UI::Shell::{
        IFolderView2, IShellBrowser, IShellItem, IShellWindows, IWebBrowser2, SID_STopLevelBrowser,
        SIGDN_FILESYSPATH, SVGIO_ALLVIEW, ShellWindows,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, EVENT_OBJECT_SHOW, GetClassNameW, GetMessageW, MSG, OBJID_WINDOW, SW_HIDE, SW_SHOW,
        ShowWindow, TranslateMessage, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
    };
    use windows::core::Interface;

    /// How long a new window has to list its folder. A slow drive takes a
    /// moment; past this the window is left alone.
    const SETTLE: Duration = Duration::from_secs(5);
    const POLL: Duration = Duration::from_millis(50);
    /// Polls after the listing during which a reveal's selection may still
    /// arrive: 150 ms, too soon for a person to have clicked.
    const SELECTION_GRACE: u32 = 3;
    /// The same wait when the window is taken either way (Prime Files is the
    /// default explorer): 0.8 s, counted from when the folder has finished
    /// listing. A reveal whose selection landed after 150 ms (the Start menu's
    /// "Open file location", issue #35) came here as a plain folder, nothing
    /// selected; in a big folder (a browser's Downloads, issue #43) the
    /// selection comes only once every item is listed, which a count from the
    /// first item missed. The window is hidden meanwhile, so the wait shows no
    /// Explorer (issue #47).
    const TAKEN_SELECTION_GRACE: u32 = 16;
    /// Polls an empty folder is given before it counts as listed: an empty
    /// view and one still reading look the same.
    const EMPTY_GRACE: u32 = 40;

    static SENDER: OnceLock<async_channel::Sender<String>> = OnceLock::new();
    // Set in the resident watch, which leaves windows to a running app.
    static RESIDENT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    const RESIDENT_MUTEX: windows::core::PCWSTR = windows::core::w!("Local\\PrimeFilesExplorerWatch");
    const RESIDENT_STOP: windows::core::PCWSTR = windows::core::w!("Local\\PrimeFilesExplorerWatchStop");
    /// How often the resident watch asks whether Prime Files is still the
    /// default explorer.
    const DEFAULT_CHECK_MS: u32 = 60_000;

    pub fn run_resident() {
        use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, LPARAM, LRESULT, WAIT_OBJECT_0, WPARAM};
        use windows::Win32::System::Threading::{CreateEventW, CreateMutexW};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW,
            PostQuitMessage, QS_ALLINPUT, RegisterClassW, SetTimer, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE,
            WM_DESTROY, WM_ENDSESSION, WM_QUERYENDSESSION, WM_QUIT, WM_TIMER, WNDCLASSW,
        };

        // One per session: the app starts one at each launch, and sign-in
        // starts another.
        let Ok(_single) = (unsafe { CreateMutexW(None, true, RESIDENT_MUTEX) }) else {
            return;
        };
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return;
        }
        let Ok(stop) = (unsafe { CreateEventW(None, true, false, RESIDENT_STOP) }) else {
            return;
        };
        if !super::super::default_explorer::is_default() {
            return;
        }
        RESIDENT.store(true, std::sync::atomic::Ordering::Relaxed);
        let (sender, receiver) = async_channel::unbounded::<String>();
        if !start_hook(sender) {
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        std::thread::Builder::new()
            .name("prime-files-resident-open".into())
            .spawn(move || {
                while let Ok(path) = receiver.recv_blocking() {
                    // The app, started with the folder (or the file to
                    // select): it opens a window, or hands over to one that
                    // started meanwhile.
                    if let Err(error) = std::process::Command::new(&exe).arg(&path).spawn() {
                        tracing::warn!("could not open {path} in Prime Files: {error}");
                    }
                }
            })
            .ok();

        // A hidden top-level window, so the installer (Restart Manager) and
        // the end of the session can ask this process to close.
        unsafe extern "system" fn on_message(hwnd: windows::Win32::Foundation::HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
            match message {
                WM_QUERYENDSESSION => LRESULT(1),
                WM_ENDSESSION | WM_CLOSE => {
                    unsafe { DestroyWindow(hwnd) }.ok();
                    LRESULT(0)
                }
                WM_DESTROY => {
                    unsafe { PostQuitMessage(0) };
                    LRESULT(0)
                }
                WM_TIMER => {
                    if !super::super::default_explorer::is_default() {
                        unsafe { DestroyWindow(hwnd) }.ok();
                    }
                    LRESULT(0)
                }
                _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
            }
        }
        let class = WNDCLASSW {
            lpfnWndProc: Some(on_message),
            lpszClassName: windows::core::w!("PrimeFilesExplorerWatch"),
            ..Default::default()
        };
        unsafe { RegisterClassW(&class) };
        let Ok(hwnd) = (unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class.lpszClassName,
                windows::core::w!("Prime Files Explorer Watch"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                None,
                None,
            )
        }) else {
            return;
        };
        unsafe { SetTimer(Some(hwnd), 1, DEFAULT_CHECK_MS, None) };
        let handles: [HANDLE; 1] = [stop];
        loop {
            let woke = unsafe { MsgWaitForMultipleObjects(Some(&handles), false, u32::MAX, QS_ALLINPUT) };
            if woke == WAIT_OBJECT_0 {
                unsafe { DestroyWindow(hwnd) }.ok();
            }
            let mut message = MSG::default();
            while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                if message.message == WM_QUIT {
                    return;
                }
                unsafe {
                    _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
    }

    pub fn stop_resident() {
        use windows::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW, SetEvent};
        if let Ok(stop) = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, RESIDENT_STOP) } {
            unsafe { SetEvent(stop) }.ok();
        }
    }

    pub fn start_hook(sender: async_channel::Sender<String>) -> bool {
        if SENDER.set(sender).is_err() {
            return false;
        }
        // An out-of-context hook calls back on the thread that set it, from its
        // message loop, so it gets a thread of its own for the whole run.
        std::thread::Builder::new()
            .name("prime-files-reveal-hook".into())
            .spawn(|| unsafe {
                let hook = SetWinEventHook(
                    EVENT_OBJECT_SHOW,
                    EVENT_OBJECT_SHOW,
                    None,
                    Some(on_show),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
                if hook.is_invalid() {
                    tracing::warn!("reveal watch disabled: SetWinEventHook failed");
                    return;
                }
                let mut message = MSG::default();
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            })
            .is_ok()
    }

    unsafe extern "system" fn on_show(
        _hook: HWINEVENTHOOK,
        _event: u32,
        hwnd: HWND,
        object: i32,
        child: i32,
        _thread: u32,
        _time: u32,
    ) {
        // Every caret and menu fires this; only whole windows of Explorer's
        // folder class matter.
        if object != OBJID_WINDOW.0 || child != 0 || !super::enabled() || !is_folder_window(hwnd) {
            return;
        }
        let hwnd = hwnd.0 as isize;
        // The view fills in after the window shows. Waited for on a thread of
        // its own, so the hook keeps answering meanwhile.
        let spawned = std::thread::Builder::new()
            .name("prime-files-reveal".into())
            .spawn(move || watch_window(hwnd));
        if let Err(error) = spawned {
            tracing::warn!("could not watch an Explorer window: {error}");
        }
    }

    fn is_folder_window(hwnd: HWND) -> bool {
        let mut class = [0u16; 32];
        let length = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
        String::from_utf16_lossy(&class[..length]) == "CabinetWClass"
    }

    fn watch_window(hwnd: isize) {
        // The app is running: its own hook takes the window.
        if RESIDENT.load(std::sync::atomic::Ordering::Relaxed) && crate::ipc::is_running() {
            return;
        }
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok().ok();
        // Asked once: it reads the registry, and asking on every poll
        // started a process each time.
        let taking = super::takes_folders();
        // A window that is taken either way is hidden at once, so the
        // Explorer never shows before the folder opens here (issue #47). It
        // comes back if it stays the Explorer's (This PC, a timeout).
        if taking {
            unsafe { ShowWindow(handle(hwnd), SW_HIDE) }.ok().ok();
        }
        let taken = watch_listing(hwnd, taking);
        if taking && !taken {
            unsafe { ShowWindow(handle(hwnd), SW_SHOW) }.ok().ok();
        }
        unsafe { CoUninitialize() };
    }

    fn handle(hwnd: isize) -> HWND {
        HWND(hwnd as *mut core::ffi::c_void)
    }

    // Waits for the window to list its folder and takes it when it should
    // show here. Returns whether it was taken.
    fn watch_listing(hwnd: isize, taking: bool) -> bool {
        let deadline = Instant::now() + SETTLE;
        // Polls since the folder's items stopped growing in number.
        let mut listed_for = None;
        let mut last_count = 0;
        let mut empty_for = 0;
        while Instant::now() < deadline {
            match read_window(hwnd) {
                Some((browser, View::Empty(folder))) => {
                    empty_for += 1;
                    if empty_for > EMPTY_GRACE {
                        return taking && take_folder(&browser, folder);
                    }
                }
                Some((browser, View::Selected(path))) => {
                    tracing::debug!("reveal of {path} taken from Explorer");
                    if let Some(sender) = SENDER.get()
                        && sender.send_blocking(path).is_ok()
                    {
                        // The window's own Quit: a posted WM_CLOSE is dropped
                        // by a window still finishing its navigation.
                        unsafe { browser.Quit() }.ok();
                        return true;
                    }
                    return false;
                }
                Some((browser, View::Listed(folder, count))) => {
                    // A reveal's selection lands with the listing or just
                    // after it; still none a moment later, the window was
                    // opened on a folder: it stays the user's, unless Prime
                    // Files is the default explorer. While the count grows the
                    // folder is still being read, and the wait starts over.
                    if count != last_count {
                        last_count = count;
                        listed_for = None;
                    }
                    let polls = listed_for.get_or_insert(0);
                    *polls += 1;
                    let grace = if taking { TAKEN_SELECTION_GRACE } else { SELECTION_GRACE };
                    if *polls > grace {
                        return taking && take_folder(&browser, folder);
                    }
                }
                // Not registered or not listed yet.
                _ => {}
            }
            std::thread::sleep(POLL);
        }
        false
    }

    // A window opened on a folder, taken when Prime Files is the default
    // explorer. Only folders on disk: This PC, the Control Panel and the like
    // have no path and stay in the Explorer.
    // Returns whether it was taken.
    fn take_folder(browser: &IWebBrowser2, folder: Option<String>) -> bool {
        let Some(folder) = folder else { return false };
        // A .zip the Explorer opens as a "compressed folder" has the file's
        // path: taken, it would come back here selected, never opened.
        if !std::path::Path::new(&folder).is_dir() {
            return false;
        }
        tracing::debug!("Explorer window on {folder} taken");
        if let Some(sender) = SENDER.get()
            && sender.send_blocking(folder).is_ok()
        {
            unsafe { browser.Quit() }.ok();
            return true;
        }
        false
    }

    enum View {
        /// Nothing is listed (yet), in the folder at this path.
        Empty(Option<String>),
        /// Items are listed (this many so far) and none is selected, in the
        /// folder at this path.
        Listed(Option<String>, i32),
        /// The first selected item, as a path.
        Selected(String),
    }

    // What the Explorer window `hwnd` shows, once it has listed its folder.
    fn read_window(hwnd: isize) -> Option<(IWebBrowser2, View)> {
        let windows: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) }.ok()?;
        let count = unsafe { windows.Count() }.ok()?;
        for index in 0..count {
            let Ok(item) = (unsafe { windows.Item(&VARIANT::from(index)) }) else { continue };
            let Ok(browser) = item.cast::<IWebBrowser2>() else { continue };
            if !unsafe { browser.HWND() }.is_ok_and(|handle| handle.0 as isize == hwnd) {
                continue;
            }
            let provider: IServiceProvider = browser.cast().ok()?;
            let shell_browser: IShellBrowser = unsafe { provider.QueryService(&SID_STopLevelBrowser) }.ok()?;
            let view: IFolderView2 = unsafe { shell_browser.QueryActiveShellView() }.ok()?.cast().ok()?;
            let folder = unsafe { view.GetFolder::<IShellItem>() }.ok().and_then(|folder| {
                let name = unsafe { folder.GetDisplayName(SIGDN_FILESYSPATH) }.ok()?;
                let path = unsafe { name.to_string() }.ok();
                unsafe { CoTaskMemFree(Some(name.0 as _)) };
                path
            });
            let count = unsafe { view.ItemCount(SVGIO_ALLVIEW) }.ok()?;
            if count == 0 {
                return Some((browser, View::Empty(folder)));
            }
            let selection = unsafe { view.GetSelection(false) }.ok();
            let first = selection
                .filter(|selection| unsafe { selection.GetCount() }.is_ok_and(|count| count > 0))
                .and_then(|selection| unsafe { selection.GetItemAt(0) }.ok());
            let Some(first) = first else {
                return Some((browser, View::Listed(folder, count)));
            };
            let name = unsafe { first.GetDisplayName(SIGDN_FILESYSPATH) }.ok()?;
            let path = unsafe { name.to_string() }.ok();
            unsafe { CoTaskMemFree(Some(name.0 as _)) };
            return Some((browser, View::Selected(path?)));
        }
        None
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn start_hook(_sender: async_channel::Sender<String>) -> bool {
        false
    }

    pub fn run_resident() {}

    pub fn stop_resident() {}
}
