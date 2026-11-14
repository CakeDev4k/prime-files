//! Prime Files' windows, listed among the Shell's folder windows
//! (`IShellWindows`), so "show this file in its folder" from other programs
//! finds them and says which file (issue #56).
//!
//! A browser's "Show in folder", the Start menu's "Open file location" and
//! most "Reveal" commands call `SHOpenFolderAndSelectItems`. The Shell looks
//! for a window registered on the folder; with none, it opens the folder —
//! through Prime Files' folder verb when it is the default — and waits for a
//! window to register there. Then it asks that window to select the file.
//! Prime Files' windows were never registered, so the folder opened with
//! nothing selected.
//!
//! What the Shell asks, seen on 14/11/2026 by logging every call on the
//! registered object (Windows 11 26200):
//! 1. The window must be registered as `SWC_BROWSER` (as `SWC_EXPLORER` it
//!    is never found) and be a real top-level window (a message-only one is
//!    not found either).
//! 2. It asks the object for `IWebBrowserApp`, reads `HWND`, then
//!    `Document`.
//! 3. On the document it asks `QueryService(SID_SFolderView, IShellView)`,
//!    and calls `IShellView::SelectItem` with the file, relative to the
//!    folder.
//!
//! The object is a stand-in for an Explorer window that knows only that.
//! It lives on a thread of its own: the Shell's calls arrive on the thread
//! that registered it, and registering and telling the Shell where a window
//! is are calls into explorer.exe, which must never hold the window's own
//! thread when the Explorer hangs.

use std::path::PathBuf;

/// The Shell asked a window to select these items in the folder it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    /// The folder the window was registered on.
    pub folder: PathBuf,
    /// The items to select, as paths.
    pub items: Vec<PathBuf>,
}

/// A window's place among the Shell's folder windows, for as long as it is
/// held.
pub struct ShellWindow {
    hwnd: i64,
}

impl ShellWindow {
    /// Registers the window `hwnd`, showing `folder`. What the Shell asks it
    /// to select goes to `sink`.
    pub fn register(hwnd: i64, folder: &str, sink: async_channel::Sender<Selected>) -> Self {
        platform::send(platform::Command::Register { hwnd, folder: folder.to_string(), sink });
        Self { hwnd }
    }

    /// The window now shows `folder`: where the Shell finds it.
    pub fn navigate(&self, folder: &str) {
        platform::send(platform::Command::Navigate { hwnd: self.hwnd, folder: folder.to_string() });
    }
}

impl Drop for ShellWindow {
    fn drop(&mut self) {
        platform::send(platform::Command::Revoke { hwnd: self.hwnd });
    }
}

#[cfg(not(windows))]
mod platform {
    pub enum Command {
        Register { hwnd: i64, folder: String, sink: async_channel::Sender<super::Selected> },
        Navigate { hwnd: i64, folder: String },
        Revoke { hwnd: i64 },
    }

    pub fn send(_command: Command) {}
}

#[cfg(windows)]
#[allow(unsafe_code, non_snake_case)]
mod platform {
    use std::cell::RefCell;
    use std::collections::{HashMap, VecDeque};
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::{Mutex, OnceLock};

    use windows::Win32::Foundation::{E_NOINTERFACE, E_NOTIMPL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, DISPATCH_FLAGS,
        DISPPARAMS, EXCEPINFO, IDispatch, IDispatch_Impl, IServiceProvider, IServiceProvider_Impl, IStream, ITypeInfo,
    };
    use windows::Win32::System::Ole::{IOleWindow, IOleWindow_Impl, OLEMENUGROUPWIDTHS};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::System::Variant::{InitVariantFromBuffer, VARIANT, VariantClear};
    use windows::Win32::UI::Controls::{LPFNSVADDPROPSHEETPAGE, TBBUTTON};
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{
        _SVGIO, FOLDERSETTINGS, IFolderView, IFolderView_Impl, ILCombine, ILFree, ILGetSize, IShellBrowser,
        IShellBrowser_Impl, IShellView, IShellView_Impl, IShellWindows, IWebBrowser_Impl, IWebBrowser2,
        IWebBrowser2_Impl, IWebBrowserApp_Impl, SHGetNameFromIDList, SHParseDisplayName, SID_STopLevelBrowser,
        SIGDN_FILESYSPATH, SWC_BROWSER, ShellWindows,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, HMENU, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, TranslateMessage,
        WM_APP,
    };
    use windows::core::{BOOL, GUID, HRESULT, Interface, PCWSTR, Ref, Result, implement};
    use windows_core::IUnknownImpl as _;

    use super::Selected;

    // `SID_SShellBrowser`, the service the view's own code asks for; the
    // same GUID as `IShellBrowser`.
    const SID_SSHELL_BROWSER: GUID = IShellBrowser::IID;

    pub enum Command {
        Register { hwnd: i64, folder: String, sink: async_channel::Sender<Selected> },
        Navigate { hwnd: i64, folder: String },
        Revoke { hwnd: i64 },
    }

    // The thread the registrations live on, and its queue.
    struct Thread {
        id: u32,
        queue: Mutex<VecDeque<Command>>,
    }

    static THREAD: OnceLock<Option<Thread>> = OnceLock::new();

    fn thread() -> Option<&'static Thread> {
        THREAD
            .get_or_init(|| {
                let (ready, id) = std::sync::mpsc::channel();
                let spawned = std::thread::Builder::new().name("prime-files-shell-windows".into()).spawn(move || {
                    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok().ok();
                    // A thread has a message queue only once it asks for one.
                    let mut message = MSG::default();
                    unsafe { PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE) }.ok().ok();
                    ready.send(unsafe { GetCurrentThreadId() }).ok();
                    run();
                });
                if let Err(error) = spawned {
                    tracing::warn!("could not start the shell windows thread: {error}");
                    return None;
                }
                Some(Thread { id: id.recv().ok()?, queue: Mutex::new(VecDeque::new()) })
            })
            .as_ref()
    }

    pub fn send(command: Command) {
        let Some(thread) = thread() else {
            return;
        };
        if let Ok(mut queue) = thread.queue.lock() {
            queue.push_back(command);
        }
        unsafe { PostThreadMessageW(thread.id, WM_APP, WPARAM(0), LPARAM(0)) }.ok();
    }

    // The thread's loop: the Shell's calls come as messages, the app's
    // commands as `WM_APP`.
    fn run() {
        let mut registered: HashMap<i64, Registration> = HashMap::new();
        let mut message = MSG::default();
        while unsafe { GetMessageW(&mut message, None, 0, 0) }.as_bool() {
            if message.hwnd.is_invalid() && message.message == WM_APP {
                let commands: Vec<Command> = THREAD
                    .get()
                    .and_then(Option::as_ref)
                    .and_then(|thread| thread.queue.lock().ok().map(|mut queue| queue.drain(..).collect()))
                    .unwrap_or_default();
                for command in commands {
                    match command {
                        Command::Register { hwnd, folder, sink } => {
                            registered.remove(&hwnd);
                            match Registration::register(hwnd, &folder, sink) {
                                Some(registration) => {
                                    registered.insert(hwnd, registration);
                                }
                                None => tracing::debug!("could not list the window among the Shell's"),
                            }
                        }
                        Command::Navigate { hwnd, folder } => {
                            if let Some(registration) = registered.get(&hwnd) {
                                registration.navigate(&folder);
                            }
                        }
                        Command::Revoke { hwnd } => {
                            registered.remove(&hwnd);
                        }
                    }
                }
                continue;
            }
            unsafe {
                _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    struct Registration {
        windows: IShellWindows,
        cookie: i32,
        // The browser's own record of the folder, shared with it.
        folder: Rc<RefCell<Option<Pidl>>>,
        _browser: IDispatch,
    }

    impl Registration {
        fn register(hwnd: i64, folder: &str, sink: async_channel::Sender<Selected>) -> Option<Self> {
            let windows: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) }.ok()?;
            let shown = Rc::new(RefCell::new(None));
            let browser: IDispatch =
                Browser { hwnd: HWND(hwnd as isize as *mut core::ffi::c_void), folder: shown.clone(), sink }.into();
            let cookie = unsafe { windows.Register(&browser, hwnd as i32, SWC_BROWSER) }.ok()?;
            let registration = Self { windows, cookie, folder: shown, _browser: browser };
            registration.navigate(folder);
            Some(registration)
        }

        fn navigate(&self, folder: &str) {
            let Some(pidl) = Pidl::of(folder) else {
                // Not a place the Shell knows (a tag, the Everything mode):
                // the Shell keeps the last folder, and a selection there
                // opens that folder again.
                return;
            };
            let Some(mut location) = pidl.variant() else {
                return;
            };
            *self.folder.borrow_mut() = Some(pidl);
            if let Err(error) = unsafe { self.windows.OnNavigate(self.cookie, &location) } {
                tracing::debug!("could not tell the Shell this window shows {folder}: {error}");
            }
            unsafe { VariantClear(&mut location) }.ok();
        }
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            unsafe { self.windows.Revoke(self.cookie) }.ok();
        }
    }

    /// An absolute item ID list the Shell allocated, freed on drop.
    struct Pidl(*mut ITEMIDLIST);

    impl Pidl {
        fn of(path: &str) -> Option<Self> {
            let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            let mut pidl = std::ptr::null_mut();
            unsafe { SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut pidl, 0, None) }.ok()?;
            (!pidl.is_null()).then_some(Self(pidl))
        }

        fn variant(&self) -> Option<VARIANT> {
            let size = unsafe { ILGetSize(Some(self.0)) };
            unsafe { InitVariantFromBuffer(self.0.cast(), size) }.ok()
        }
    }

    impl Drop for Pidl {
        fn drop(&mut self) {
            unsafe { ILFree(Some(self.0)) };
        }
    }

    // The path of an absolute item ID list, when it is a file on disk.
    fn path_of(pidl: *const ITEMIDLIST) -> Option<PathBuf> {
        let name = unsafe { SHGetNameFromIDList(pidl, SIGDN_FILESYSPATH) }.ok()?;
        let path = unsafe { name.to_string() }.ok();
        unsafe { CoTaskMemFree(Some(name.0 as _)) };
        path.map(PathBuf::from)
    }

    /// The stand-in browser, which is also its own document and view.
    #[implement(IWebBrowser2, IDispatch, IServiceProvider, IShellBrowser, IShellView, IFolderView, IOleWindow)]
    struct Browser {
        hwnd: HWND,
        folder: Rc<RefCell<Option<Pidl>>>,
        sink: async_channel::Sender<Selected>,
    }

    impl Browser {
        // Items named relative to the folder shown, sent as paths.
        fn select(&self, items: &[*const ITEMIDLIST]) -> Result<()> {
            let folder = self.folder.borrow();
            let Some(folder) = folder.as_ref() else {
                return Err(E_NOTIMPL.into());
            };
            let Some(folder_path) = path_of(folder.0) else {
                return Err(E_NOTIMPL.into());
            };
            let mut paths = Vec::new();
            for item in items {
                if item.is_null() {
                    continue;
                }
                let full = unsafe { ILCombine(Some(folder.0), Some(*item)) };
                if full.is_null() {
                    continue;
                }
                if let Some(path) = path_of(full) {
                    paths.push(path);
                }
                unsafe { ILFree(Some(full)) };
            }
            if !paths.is_empty() {
                self.sink.try_send(Selected { folder: folder_path, items: paths }).ok();
            }
            Ok(())
        }
    }

    impl IDispatch_Impl for Browser_Impl {
        fn GetTypeInfoCount(&self) -> Result<u32> {
            Ok(0)
        }
        fn GetTypeInfo(&self, _: u32, _: u32) -> Result<ITypeInfo> {
            Err(E_NOTIMPL.into())
        }
        fn GetIDsOfNames(&self, _: *const GUID, _: *const PCWSTR, _: u32, _: u32, _: *mut i32) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn Invoke(
            &self,
            _: i32,
            _: *const GUID,
            _: u32,
            _: DISPATCH_FLAGS,
            _: *const DISPPARAMS,
            _: *mut VARIANT,
            _: *mut EXCEPINFO,
            _: *mut u32,
        ) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
    }

    // What an Explorer window answers that the Shell reads: its window, and
    // its document (this object, whose view selects). The rest is never
    // asked of it.

    impl IWebBrowser_Impl for Browser_Impl {
        fn GoBack(&self) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn GoForward(&self) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn GoHome(&self) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn GoSearch(&self) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn Navigate(&self, _url: &windows_core::BSTR, _flags: *const windows::Win32::System::Variant::VARIANT, _targetframename: *const windows::Win32::System::Variant::VARIANT, _postdata: *const windows::Win32::System::Variant::VARIANT, _headers: *const windows::Win32::System::Variant::VARIANT) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn Refresh(&self) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn Refresh2(&self, _level: *const windows::Win32::System::Variant::VARIANT) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn Stop(&self) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn Application(&self) -> windows_core::Result<windows::Win32::System::Com::IDispatch> {
            Err(E_NOTIMPL.into())
        }
        fn Parent(&self) -> windows_core::Result<windows::Win32::System::Com::IDispatch> {
            Err(E_NOTIMPL.into())
        }
        fn Container(&self) -> windows_core::Result<windows::Win32::System::Com::IDispatch> {
            Err(E_NOTIMPL.into())
        }
        fn Document(&self) -> windows_core::Result<windows::Win32::System::Com::IDispatch> {
            Ok(self.to_object().to_interface())
        }
        fn TopLevelContainer(&self) -> windows_core::Result<windows::Win32::Foundation::VARIANT_BOOL> {
            Err(E_NOTIMPL.into())
        }
        fn Type(&self) -> windows_core::Result<windows_core::BSTR> {
            Err(E_NOTIMPL.into())
        }
        fn Left(&self) -> windows_core::Result<i32> {
            Err(E_NOTIMPL.into())
        }
        fn SetLeft(&self, _left: i32) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn Top(&self) -> windows_core::Result<i32> {
            Err(E_NOTIMPL.into())
        }
        fn SetTop(&self, _top: i32) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn Width(&self) -> windows_core::Result<i32> {
            Err(E_NOTIMPL.into())
        }
        fn SetWidth(&self, _width: i32) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn Height(&self) -> windows_core::Result<i32> {
            Err(E_NOTIMPL.into())
        }
        fn SetHeight(&self, _height: i32) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn LocationName(&self) -> windows_core::Result<windows_core::BSTR> {
            Err(E_NOTIMPL.into())
        }
        fn LocationURL(&self) -> windows_core::Result<windows_core::BSTR> {
            Err(E_NOTIMPL.into())
        }
        fn Busy(&self) -> windows_core::Result<windows::Win32::Foundation::VARIANT_BOOL> {
            Err(E_NOTIMPL.into())
        }
    }

    impl IWebBrowserApp_Impl for Browser_Impl {
        fn Quit(&self) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn ClientToWindow(&self, _pcx: *mut i32, _pcy: *mut i32) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn PutProperty(&self, _property: &windows_core::BSTR, _vtvalue: &windows::Win32::System::Variant::VARIANT) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn GetProperty(&self, _property: &windows_core::BSTR) -> windows_core::Result<windows::Win32::System::Variant::VARIANT> {
            Err(E_NOTIMPL.into())
        }
        fn Name(&self) -> windows_core::Result<windows_core::BSTR> {
            Err(E_NOTIMPL.into())
        }
        fn HWND(&self) -> windows_core::Result<windows::Win32::Foundation::SHANDLE_PTR> {
            Ok(windows::Win32::Foundation::SHANDLE_PTR(self.hwnd.0 as isize))
        }
        fn FullName(&self) -> windows_core::Result<windows_core::BSTR> {
            Err(E_NOTIMPL.into())
        }
        fn Path(&self) -> windows_core::Result<windows_core::BSTR> {
            Err(E_NOTIMPL.into())
        }
        fn Visible(&self) -> windows_core::Result<windows::Win32::Foundation::VARIANT_BOOL> {
            Ok(windows::Win32::Foundation::VARIANT_TRUE)
        }
        fn SetVisible(&self, _value: windows::Win32::Foundation::VARIANT_BOOL) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn StatusBar(&self) -> windows_core::Result<windows::Win32::Foundation::VARIANT_BOOL> {
            Err(E_NOTIMPL.into())
        }
        fn SetStatusBar(&self, _value: windows::Win32::Foundation::VARIANT_BOOL) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn StatusText(&self) -> windows_core::Result<windows_core::BSTR> {
            Err(E_NOTIMPL.into())
        }
        fn SetStatusText(&self, _statustext: &windows_core::BSTR) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn ToolBar(&self) -> windows_core::Result<i32> {
            Err(E_NOTIMPL.into())
        }
        fn SetToolBar(&self, _value: i32) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn MenuBar(&self) -> windows_core::Result<windows::Win32::Foundation::VARIANT_BOOL> {
            Err(E_NOTIMPL.into())
        }
        fn SetMenuBar(&self, _value: windows::Win32::Foundation::VARIANT_BOOL) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn FullScreen(&self) -> windows_core::Result<windows::Win32::Foundation::VARIANT_BOOL> {
            Err(E_NOTIMPL.into())
        }
        fn SetFullScreen(&self, _bfullscreen: windows::Win32::Foundation::VARIANT_BOOL) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
    }

    impl IWebBrowser2_Impl for Browser_Impl {
        fn Navigate2(&self, _url: *const windows::Win32::System::Variant::VARIANT, _flags: *const windows::Win32::System::Variant::VARIANT, _targetframename: *const windows::Win32::System::Variant::VARIANT, _postdata: *const windows::Win32::System::Variant::VARIANT, _headers: *const windows::Win32::System::Variant::VARIANT) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn QueryStatusWB(&self, _cmdid: windows::Win32::System::Ole::OLECMDID) -> windows_core::Result<windows::Win32::System::Ole::OLECMDF> {
            Err(E_NOTIMPL.into())
        }
        fn ExecWB(&self, _cmdid: windows::Win32::System::Ole::OLECMDID, _cmdexecopt: windows::Win32::System::Ole::OLECMDEXECOPT, _pvain: *const windows::Win32::System::Variant::VARIANT, _pvaout: *mut windows::Win32::System::Variant::VARIANT) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn ShowBrowserBar(&self, _pvaclsid: *const windows::Win32::System::Variant::VARIANT, _pvarshow: *const windows::Win32::System::Variant::VARIANT, _pvarsize: *const windows::Win32::System::Variant::VARIANT) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn ReadyState(&self) -> windows_core::Result<windows::Win32::System::Ole::READYSTATE> {
            Err(E_NOTIMPL.into())
        }
        fn Offline(&self) -> windows_core::Result<windows::Win32::Foundation::VARIANT_BOOL> {
            Err(E_NOTIMPL.into())
        }
        fn SetOffline(&self, _boffline: windows::Win32::Foundation::VARIANT_BOOL) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn Silent(&self) -> windows_core::Result<windows::Win32::Foundation::VARIANT_BOOL> {
            Err(E_NOTIMPL.into())
        }
        fn SetSilent(&self, _bsilent: windows::Win32::Foundation::VARIANT_BOOL) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn RegisterAsBrowser(&self) -> windows_core::Result<windows::Win32::Foundation::VARIANT_BOOL> {
            Err(E_NOTIMPL.into())
        }
        fn SetRegisterAsBrowser(&self, _bregister: windows::Win32::Foundation::VARIANT_BOOL) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn RegisterAsDropTarget(&self) -> windows_core::Result<windows::Win32::Foundation::VARIANT_BOOL> {
            Err(E_NOTIMPL.into())
        }
        fn SetRegisterAsDropTarget(&self, _bregister: windows::Win32::Foundation::VARIANT_BOOL) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn TheaterMode(&self) -> windows_core::Result<windows::Win32::Foundation::VARIANT_BOOL> {
            Err(E_NOTIMPL.into())
        }
        fn SetTheaterMode(&self, _bregister: windows::Win32::Foundation::VARIANT_BOOL) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn AddressBar(&self) -> windows_core::Result<windows::Win32::Foundation::VARIANT_BOOL> {
            Err(E_NOTIMPL.into())
        }
        fn SetAddressBar(&self, _value: windows::Win32::Foundation::VARIANT_BOOL) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn Resizable(&self) -> windows_core::Result<windows::Win32::Foundation::VARIANT_BOOL> {
            Err(E_NOTIMPL.into())
        }
        fn SetResizable(&self, _value: windows::Win32::Foundation::VARIANT_BOOL) -> windows_core::Result<()> {
            Err(E_NOTIMPL.into())
        }
    }

    impl IServiceProvider_Impl for Browser_Impl {
        fn QueryService(&self, service: *const GUID, iid: *const GUID, out: *mut *mut core::ffi::c_void) -> Result<()> {
            if out.is_null() || service.is_null() {
                return Err(E_NOINTERFACE.into());
            }
            let service = unsafe { *service };
            if service != SID_STopLevelBrowser && service != SID_SSHELL_BROWSER && service != IFolderView::IID {
                unsafe { *out = std::ptr::null_mut() };
                return Err(E_NOINTERFACE.into());
            }
            let unknown: windows::core::IUnknown = self.to_object().to_interface();
            unsafe { unknown.query(iid, out) }.ok()
        }
    }

    impl IOleWindow_Impl for Browser_Impl {
        fn GetWindow(&self) -> Result<HWND> {
            Ok(self.hwnd)
        }
        fn ContextSensitiveHelp(&self, _: BOOL) -> Result<()> {
            Ok(())
        }
    }

    impl IShellBrowser_Impl for Browser_Impl {
        fn InsertMenusSB(&self, _: HMENU, _: *mut OLEMENUGROUPWIDTHS) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn SetMenuSB(&self, _: HMENU, _: isize, _: HWND) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn RemoveMenusSB(&self, _: HMENU) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn SetStatusTextSB(&self, _: &PCWSTR) -> Result<()> {
            Ok(())
        }
        fn EnableModelessSB(&self, _: BOOL) -> Result<()> {
            Ok(())
        }
        fn TranslateAcceleratorSB(&self, _: *const MSG, _: u16) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn BrowseObject(&self, _: *const ITEMIDLIST, _: u32) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn GetViewStateStream(&self, _: u32) -> Result<IStream> {
            Err(E_NOTIMPL.into())
        }
        fn GetControlWindow(&self, _: u32) -> Result<HWND> {
            Err(E_NOTIMPL.into())
        }
        fn SendControlMsg(&self, _: u32, _: u32, _: WPARAM, _: LPARAM, _: *mut LRESULT) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn QueryActiveShellView(&self) -> Result<IShellView> {
            Ok(self.to_object().to_interface())
        }
        fn OnViewWindowActive(&self, _: Ref<'_, IShellView>) -> Result<()> {
            Ok(())
        }
        fn SetToolbarItems(&self, _: *const TBBUTTON, _: u32, _: u32) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
    }

    impl IShellView_Impl for Browser_Impl {
        fn TranslateAccelerator(&self, _: *const MSG) -> HRESULT {
            windows::Win32::Foundation::S_FALSE
        }
        fn EnableModeless(&self, _: BOOL) -> Result<()> {
            Ok(())
        }
        fn UIActivate(&self, _: u32) -> Result<()> {
            Ok(())
        }
        fn Refresh(&self) -> Result<()> {
            Ok(())
        }
        fn CreateViewWindow(&self, _: Ref<'_, IShellView>, _: *const FOLDERSETTINGS, _: Ref<'_, IShellBrowser>, _: *const RECT) -> Result<HWND> {
            Err(E_NOTIMPL.into())
        }
        fn DestroyViewWindow(&self) -> Result<()> {
            Ok(())
        }
        fn GetCurrentInfo(&self) -> Result<FOLDERSETTINGS> {
            Err(E_NOTIMPL.into())
        }
        fn AddPropertySheetPages(&self, _: u32, _: LPFNSVADDPROPSHEETPAGE, _: LPARAM) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn SaveViewState(&self) -> Result<()> {
            Ok(())
        }
        fn SelectItem(&self, item: *const ITEMIDLIST, flags: u32) -> Result<()> {
            // SVSI_SELECT; a call that only deselects (0) or focuses is
            // nothing to show.
            if flags & 1 == 0 {
                return Ok(());
            }
            self.select(&[item])
        }
        fn GetItemObject(&self, _: &_SVGIO, _: *const GUID, _: *mut *mut core::ffi::c_void) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
    }

    impl IFolderView_Impl for Browser_Impl {
        fn GetCurrentViewMode(&self) -> Result<u32> {
            Err(E_NOTIMPL.into())
        }
        fn SetCurrentViewMode(&self, _: u32) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn GetFolder(&self, _: *const GUID, _: *mut *mut core::ffi::c_void) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn Item(&self, _: i32) -> Result<*mut ITEMIDLIST> {
            Err(E_NOTIMPL.into())
        }
        fn ItemCount(&self, _: &_SVGIO) -> Result<i32> {
            Ok(0)
        }
        fn Items(&self, _: &_SVGIO, _: *const GUID, _: *mut *mut core::ffi::c_void) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn GetSelectionMarkedItem(&self) -> Result<i32> {
            Err(E_NOTIMPL.into())
        }
        fn GetFocusedItem(&self) -> Result<i32> {
            Err(E_NOTIMPL.into())
        }
        fn GetItemPosition(&self, _: *const ITEMIDLIST) -> Result<POINT> {
            Err(E_NOTIMPL.into())
        }
        fn GetSpacing(&self, _: *mut POINT) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn GetDefaultSpacing(&self) -> Result<POINT> {
            Err(E_NOTIMPL.into())
        }
        fn GetAutoArrange(&self) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn SelectItem(&self, _: i32, _: u32) -> Result<()> {
            Err(E_NOTIMPL.into())
        }
        fn SelectAndPositionItems(&self, count: u32, items: *const *const ITEMIDLIST, _: *const POINT, flags: u32) -> Result<()> {
            if items.is_null() || flags & 1 == 0 {
                return Ok(());
            }
            let items = unsafe { std::slice::from_raw_parts(items, count as usize) };
            self.select(items)
        }
    }

    #[cfg(test)]
    mod tests {
        use std::time::{Duration, Instant};

        use windows::Win32::System::Com::{CoUninitialize};
        use windows::Win32::UI::Shell::{ILFindLastID, SHOpenFolderAndSelectItems, SWFO_NEEDDISPATCH};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, WS_VISIBLE,
        };
        use windows::core::w;

        use super::*;
        use crate::shell_window::ShellWindow;

        // The Shell, asked to show a file in a folder one of our windows is
        // registered on, asks that window to select it, and opens nothing.
        // Ignored: it talks to the running Explorer. Its window is real, as
        // the Shell wants, but off the screen and out of the taskbar; and
        // the Shell is only asked once it has said it finds ours, since a
        // Shell that does not find it opens the folder in the default file
        // manager.
        // cargo test -p prime-files-pages --lib the_shell_selects_in_a_registered_window -- --ignored --nocapture
        #[test]
        #[ignore = "talks to the running Explorer"]
        fn the_shell_selects_in_a_registered_window() {
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok().unwrap();
            let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target");
            let dir = tempfile::Builder::new().prefix("shell-window").tempdir_in(&base).unwrap();
            let folder = dir.path().canonicalize().unwrap().to_string_lossy().trim_start_matches(r"\\?\").to_string();
            let file = std::path::PathBuf::from(&folder).join("alvo.txt");
            std::fs::write(&file, "x").unwrap();
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                    w!("STATIC"),
                    w!("prime-files-test"),
                    WS_POPUP | WS_VISIBLE,
                    -32000,
                    -32000,
                    10,
                    10,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .unwrap();
            let (sink, selected) = async_channel::unbounded();
            // Registered on another folder, then moved: what navigation does.
            let window = ShellWindow::register(hwnd.0 as i64, &base.canonicalize().unwrap().to_string_lossy(), sink);
            window.navigate(&folder);

            let windows: IShellWindows = unsafe { CoCreateInstance(&ShellWindows, None, CLSCTX_ALL) }.unwrap();
            let location = Pidl::of(&folder).unwrap().variant().unwrap();
            let empty = VARIANT::default();
            let until = Instant::now() + Duration::from_secs(3);
            let mut found = 0;
            while found as isize != hwnd.0 as isize && Instant::now() < until {
                found = 0;
                unsafe { windows.FindWindowSW(&location, &empty, SWC_BROWSER, &mut found, SWFO_NEEDDISPATCH) }.ok();
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(found as isize, hwnd.0 as isize, "the Shell does not find the registered window");

            let folder_pidl = Pidl::of(&folder).unwrap();
            let file_pidl = Pidl::of(&file.to_string_lossy()).unwrap();
            let result = unsafe { SHOpenFolderAndSelectItems(folder_pidl.0, Some(&[ILFindLastID(file_pidl.0) as *const _]), 0) };
            let got = selected.recv_blocking().ok();
            drop(window);
            unsafe { DestroyWindow(hwnd) }.ok();
            unsafe { CoUninitialize() };
            result.unwrap();
            assert_eq!(got.map(|got| got.items), Some(vec![file]));
        }
    }
}
