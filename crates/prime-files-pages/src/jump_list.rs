//! The folders pinned to Quick access in Prime Files' own Jump List, the menu
//! of its taskbar and Start menu icon (issue #46), as Explorer lists them in
//! its own. Each entry starts Prime Files with the folder, which the running
//! window takes as a new tab.
//!
//! Rewritten only when the pinned folders change: the sidebar reads them
//! every few seconds, and the list is the Shell's to keep between runs.

use std::sync::Mutex;

// What the list was last written with: (name, path) in order.
static WRITTEN: Mutex<Option<Vec<(String, String)>>> = Mutex::new(None);

/// Puts `pinned` (name and folder) in the Jump List, unless that is what it
/// already holds. Writes on a thread of its own, with COM set up there.
pub(crate) fn update(pinned: Vec<(String, String)>) {
    {
        let Ok(mut written) = WRITTEN.lock() else {
            return;
        };
        if written.as_ref() == Some(&pinned) {
            return;
        }
        *written = Some(pinned.clone());
    }
    let title = prime_files_core::i18n::tr(prime_files_core::k!("Fixadas")).to_string();
    let spawned = std::thread::Builder::new()
        .name("prime-files-jump-list".into())
        .spawn(move || {
            if let Err(error) = platform::write(&title, &pinned) {
                tracing::warn!("could not write the jump list: {error}");
                // Tried again on the next read of the places.
                if let Ok(mut written) = WRITTEN.lock() {
                    *written = None;
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!("could not start writing the jump list: {error}");
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    //! The Jump List has no safe binding.

    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{
        DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, ShellLink,
    };
    use windows::core::{HSTRING, Interface};

    // `PKEY_Title`, the text a Jump List shows for a link. Spelled out here
    // rather than taken from `Win32_Storage_EnhancedStorage`, a whole feature
    // of the crate for one constant.
    const TITLE: PROPERTYKEY = PROPERTYKEY {
        fmtid: windows::core::GUID::from_u128(0xf29f85e0_4ff9_1068_ab91_08002b27b3d9),
        pid: 2,
    };

    // Windows' icon library; its fourth icon is the plain folder.
    fn icons() -> String {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_string());
        format!(r"{root}\System32\imageres.dll")
    }

    pub fn write(title: &str, pinned: &[(String, String)]) -> windows::core::Result<()> {
        let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        let result = build(title, pinned);
        if initialized {
            unsafe { CoUninitialize() };
        }
        result
    }

    fn build(title: &str, pinned: &[(String, String)]) -> windows::core::Result<()> {
        let exe = std::env::current_exe().map_err(|error| windows::core::Error::new(windows::core::HRESULT(-1), error.to_string()))?;
        let list: ICustomDestinationList = unsafe { CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER) }?;
        let mut slots = 0u32;
        let _removed: IObjectArray = unsafe { list.BeginList(&mut slots) }?;
        let collection: IObjectCollection =
            unsafe { CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER) }?;
        for (name, path) in pinned.iter().take(slots.max(1) as usize) {
            let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }?;
            unsafe {
                link.SetPath(&HSTRING::from(exe.as_os_str()))?;
                link.SetArguments(&HSTRING::from(format!("\"{path}\"")))?;
                // The folder's own icon, as Explorer's list shows it.
                link.SetIconLocation(&HSTRING::from(icons()), 3)?;
                link.SetDescription(&HSTRING::from(path.as_str()))?;
            }
            let store: IPropertyStore = link.cast()?;
            unsafe {
                store.SetValue(&TITLE, &PROPVARIANT::from(name.as_str()))?;
                store.Commit()?;
            }
            unsafe { collection.AddObject(&link) }?;
        }
        let items: IObjectArray = collection.cast()?;
        if !pinned.is_empty() {
            unsafe { list.AppendCategory(&HSTRING::from(title), &items) }?;
        }
        unsafe { list.CommitList() }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn write(_title: &str, _pinned: &[(String, String)]) -> Result<(), String> {
        Ok(())
    }
}
