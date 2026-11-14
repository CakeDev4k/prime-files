//! "Convert with Filee": hands the selected files to Filee
//! (<https://filee.sh>), a file converter, as `Filee.exe --convert <paths>`.
//!
//! From PR #54 (byKoro), adapted: the entry shows only when Filee is
//! installed, and finding it — a dozen folder and registry reads — runs on
//! a thread of its own, never while a menu or a key waits.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gpui::Context;
use prime_files_core::{t, tf};

use super::ExplorerPane;
use super::types::StatusLevel;

/// Filee is looked for again at most this often, so installing it while
/// the app runs shows the entry without a restart.
const LOOK_AGAIN_AFTER: Duration = Duration::from_secs(60);

struct Found {
    exe: Option<PathBuf>,
    at: Instant,
}

static FOUND: Mutex<Option<Found>> = Mutex::new(None);
static LOOKING: AtomicBool = AtomicBool::new(false);

/// Where Filee is, as last found; `None` when it is not installed or the
/// first look has not ended. Starts a look in the background when the last
/// is old.
pub(crate) fn installed() -> Option<PathBuf> {
    let (exe, stale) = match FOUND.lock() {
        Ok(found) => match found.as_ref() {
            Some(found) => (found.exe.clone(), found.at.elapsed() > LOOK_AGAIN_AFTER),
            None => (None, true),
        },
        Err(_) => (None, false),
    };
    if stale && !LOOKING.swap(true, Ordering::AcqRel) {
        let spawned = std::thread::Builder::new().name("prime-files-find-filee".into()).spawn(|| {
            let exe = find();
            if let Ok(mut found) = FOUND.lock() {
                *found = Some(Found { exe, at: Instant::now() });
            }
            LOOKING.store(false, Ordering::Release);
        });
        if spawned.is_err() {
            LOOKING.store(false, Ordering::Release);
        }
    }
    exe
}

/// Starts the first look, so the first menu already knows.
pub(crate) fn look_soon() {
    installed();
}

impl ExplorerPane {
    /// Sends the selected files to Filee.
    pub(crate) fn convert_selection_with_filee(&mut self, cx: &mut Context<Self>) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            self.set_status(StatusLevel::Info, t!("Selecione ao menos um arquivo para converter").to_string());
            cx.notify();
            return;
        }
        let Some(exe) = installed() else {
            self.set_status(StatusLevel::Error, t!("O Filee não está instalado").to_string());
            cx.notify();
            return;
        };
        let mut command = std::process::Command::new(exe);
        command.arg("--convert").args(&paths);
        match command.spawn() {
            Ok(_) => self.clear_status(),
            Err(error) => {
                self.set_status(StatusLevel::Error, tf!("Não foi possível abrir o programa: {error}", error = error));
            }
        }
        cx.notify();
    }
}

// Filee.exe in its install folders, the registry, or beside the `filee`
// command on PATH.
#[allow(clippy::disallowed_methods)]
fn find() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(folder) = std::env::var_os(variable) {
            candidates.push(PathBuf::from(folder).join("Filee").join("Filee.exe"));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        candidates.push(local.join("Programs").join("Filee").join("Filee.exe"));
        candidates.push(local.join("Filee").join("Filee.exe"));
    }
    if let Some(found) = candidates.into_iter().find(|path| path.is_file()) {
        return Some(found);
    }
    #[cfg(windows)]
    if let Some(found) = platform::registered() {
        return Some(found);
    }
    let path = std::env::var_os("PATH")?;
    for folder in std::env::split_paths(&path) {
        let exe = folder.join("Filee.exe");
        if exe.is_file() {
            return Some(exe);
        }
        // The installer puts Filee's `cli` folder on PATH for the `filee`
        // command; the program is in the folder above.
        let is_cli = folder.file_name().is_some_and(|name| name.eq_ignore_ascii_case("cli"));
        let has_command = ["filee.cmd", "filee.bat", "filee"].iter().any(|name| folder.join(name).is_file());
        if (is_cli || has_command)
            && let Some(parent) = folder.parent()
            && parent.join("Filee.exe").is_file()
        {
            return Some(parent.join("Filee.exe"));
        }
    }
    None
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    use std::path::PathBuf;

    use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::{HSTRING, PCWSTR};

    // A text value of the registry; `name` `None` is the key's default.
    fn value(root: HKEY, key: &str, name: Option<&str>) -> Option<String> {
        let key = HSTRING::from(key);
        let name = name.map(HSTRING::from);
        let name = name.as_ref().map_or(PCWSTR::null(), |name| PCWSTR(name.as_ptr()));
        let mut buffer = [0u16; 1024];
        let mut bytes = (buffer.len() * 2) as u32;
        let read = unsafe {
            RegGetValueW(root, PCWSTR(key.as_ptr()), name, RRF_RT_REG_SZ, None, Some(buffer.as_mut_ptr().cast()), Some(&mut bytes))
        };
        if read.is_err() || bytes <= 2 {
            return None;
        }
        let units = (bytes as usize / 2).saturating_sub(1);
        let text = String::from_utf16_lossy(&buffer[..units]);
        // An icon's value ends in its index: `…\Filee.exe,0`.
        let text = match text.rsplit_once(',') {
            Some((path, index)) if index.trim().parse::<i32>().is_ok() => path.to_string(),
            _ => text,
        };
        Some(text.trim().trim_matches('"').to_string())
    }

    // The program a value names: the exe itself, or the folder holding it.
    #[allow(clippy::disallowed_methods)]
    fn program(text: &str) -> Option<PathBuf> {
        let path = PathBuf::from(text);
        if path.is_file() {
            return Some(path);
        }
        let inside = path.join("Filee.exe");
        inside.is_file().then_some(inside)
    }

    pub fn registered() -> Option<PathBuf> {
        const UNINSTALL: &[&str] = &[
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{C8686E06-8327-40A2-B0F7-3DEDD61D9248}_is1",
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Filee_is1",
        ];
        for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            let app_path = value(root, r"Software\Microsoft\Windows\CurrentVersion\App Paths\Filee.exe", None);
            if let Some(found) = app_path.as_deref().and_then(program) {
                return Some(found);
            }
            // The verb Filee adds to every file: `"C:\…\Filee.exe" …`.
            if let Some(command) = value(root, r"Software\Classes\*\shell\Filee\command", None) {
                let exe = command.split('"').find(|part| part.to_lowercase().ends_with(".exe")).unwrap_or(&command);
                if let Some(found) = program(exe.trim()) {
                    return Some(found);
                }
            }
            for key in UNINSTALL {
                for name in ["InstallLocation", "DisplayIcon"] {
                    if let Some(found) = value(root, key, Some(name)).as_deref().and_then(program) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }
}
