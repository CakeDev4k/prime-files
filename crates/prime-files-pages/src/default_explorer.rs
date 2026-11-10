//! Making Prime Files the folder handler of the current Windows user, from the
//! settings page. The same registry entries `integrations\windows\tornar-padrao.bat`
//! writes, plus Win+E and the taskbar's Explorer button.
//!
//! All of it lives under `HKCU\Software\Classes`, so it needs no administrator
//! and touches no other account; turning it off deletes only what was added
//! here. Written through `reg.exe` rather than the registry API so each entry
//! reads exactly like the line in the `.bat` that a person can check by hand.

use prime_files_core::t;
use std::io;
use std::path::Path;
use std::process::Command;

// The verb added under `Directory\shell` and `Drive\shell`; the key's default
// value names it, which makes it the double-click action.
const VERB: &str = "prime-files";
// The verb's name up to 0.3.9. A registry written by one of those versions
// still has it: it counts as ours, and is removed when the new one is written
// or the choice is undone, so no dead "Open in" entry is left behind.
const LEGACY_VERB: &str = "nohrs";
const FOLDER_CLASSES: [&str; 2] = ["Directory", "Drive"];
// What Win+E and the taskbar's Explorer button launch ("This PC" in a new
// window). A `command` with an empty `DelegateExecute` under it replaces the
// COM handler with a plain program for this user, which is how replacement
// file managers take those two over.
const NEW_WINDOW_KEY: &str =
    r"HKCU\Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}\shell\opennewwindow";
// The same object's default verb. Win+E asks for `opennewwindow`, but the
// File Explorer pinned to the taskbar (and the Start menu's) is a shortcut to
// the object itself, which runs `open`: with only the first taken over, the
// taskbar button kept opening Explorer (Prime Files issue #4).
const OPEN_KEY: &str = r"HKCU\Software\Classes\CLSID\{52205fd8-5dfb-447d-801a-d0b52f2e83e1}\shell\open";

// Desktop icons that are shell objects, not folders: double-clicked, they
// run their own `open` verb, which went to Explorer even with Prime Files the
// default (issue #42). Given a command for this user, they open here, at the
// address the app knows them by.
const SHELL_OBJECTS: [(&str, &str); 2] = [
    (
        r"HKCU\Software\Classes\CLSID\{645FF040-5081-101B-9F08-00AA002F954E}\shell\open",
        "::{645FF040-5081-101B-9F08-00AA002F954E}",
    ),
    (
        r"HKCU\Software\Classes\CLSID\{20D04FE0-3AEA-1069-A2D8-08002B30309D}\shell\open",
        "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}",
    ),
];

fn reg(args: &[&str]) -> io::Result<std::process::Output> {
    let mut command = Command::new("reg.exe");
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console flashing up over the window for each call.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.output()
}

fn reg_ok(args: &[&str]) -> io::Result<()> {
    let output = reg(args)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(String::from_utf8_lossy(&output.stderr).trim().to_string()))
    }
}

/// Whether folders currently open in Prime Files for this user.
pub fn is_default() -> bool {
    reg(&["query", r"HKCU\Software\Classes\Directory\shell", "/ve"])
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|line| {
                        let line = line.trim_end();
                        line.contains("REG_SZ") && (line.ends_with(VERB) || line.ends_with(LEGACY_VERB))
                    })
        })
        .unwrap_or(false)
}

/// Makes `exe` open folders, drives, Win+E and the taskbar's Explorer button.
pub fn enable(exe: &Path) -> io::Result<()> {
    let exe = exe.to_string_lossy();
    let quoted = format!("\"{exe}\"");
    for class in FOLDER_CLASSES {
        let shell = format!(r"HKCU\Software\Classes\{class}\shell");
        let verb = format!(r"{shell}\{VERB}");
        reg_ok(&["add", &verb, "/ve", "/d", t!("Abrir no Prime Files"), "/f"])?;
        reg_ok(&["add", &verb, "/v", "Icon", "/d", &quoted, "/f"])?;
        reg_ok(&["add", &format!(r"{verb}\command"), "/ve", "/d", &format!("{quoted} \"%1\""), "/f"])?;
        reg_ok(&["add", &shell, "/ve", "/d", VERB, "/f"])?;
        // Not `reg_ok`: there is usually no old verb to delete.
        reg(&["delete", &format!(r"{shell}\{LEGACY_VERB}"), "/f"])?;
    }
    launcher_verbs(&quoted)
}

// Without an argument Prime Files brings its open window forward, or opens one.
fn launcher_verbs(quoted: &str) -> io::Result<()> {
    for key in [NEW_WINDOW_KEY, OPEN_KEY] {
        let command = format!(r"{key}\command");
        reg_ok(&["add", &command, "/ve", "/d", quoted, "/f"])?;
        reg_ok(&["add", &command, "/v", "DelegateExecute", "/d", "", "/f"])?;
    }
    shell_object_verbs(quoted)
}

// The Recycle Bin and This PC on the desktop, opened by `program` (quoted).
fn shell_object_verbs(program: &str) -> io::Result<()> {
    for (key, address) in SHELL_OBJECTS {
        let command = format!(r"{key}\command");
        reg_ok(&["add", &command, "/ve", "/d", &format!("{program} \"{address}\""), "/f"])?;
        reg_ok(&["add", &command, "/v", "DelegateExecute", "/d", "", "/f"])?;
    }
    Ok(())
}

/// For a copy made the default before the taskbar's verb was taken over
/// too: gives `open` the program `opennewwindow` already names. Never points
/// either at the running executable, which may be a test copy; does nothing
/// while Prime Files is not the default or `open` already has a command.
pub fn repair() -> io::Result<()> {
    // Only the installed copy: a build run from the source tree, or a test
    // copy, must not change what the person's taskbar opens.
    let installed = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|folder| folder.join("unins000.exe").is_file()))
        .unwrap_or(false);
    if !installed || !is_default() {
        return Ok(());
    }
    let command_of = |key: &str| -> Option<String> {
        let output = reg(&["query", &format!(r"{key}\command"), "/ve"]).ok()?;
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.split_once("REG_SZ").map(|(_, value)| value.trim().to_string()))
            .filter(|value| !value.is_empty())
    };
    if !is_ours(&format!(r"{NEW_WINDOW_KEY}\command")) {
        return Ok(());
    }
    let Some(program) = command_of(NEW_WINDOW_KEY) else {
        return Ok(());
    };
    // Made default before the desktop's Recycle Bin and This PC were
    // taken over too.
    if SHELL_OBJECTS.iter().any(|(key, _)| command_of(key).is_none()) {
        shell_object_verbs(&program)?;
    }
    if command_of(OPEN_KEY).is_some() {
        return Ok(());
    }
    let command = format!(r"{OPEN_KEY}\command");
    reg_ok(&["add", &command, "/ve", "/d", &program, "/f"])?;
    reg_ok(&["add", &command, "/v", "DelegateExecute", "/d", "", "/f"])
}

/// Gives folders back to Explorer, removing only what [`enable`] added.
pub fn disable() -> io::Result<()> {
    for class in FOLDER_CLASSES {
        let shell = format!(r"HKCU\Software\Classes\{class}\shell");
        // Not `reg_ok`: a key that is not there fails the delete, and is
        // already undone.
        reg(&["delete", &format!(r"{shell}\{VERB}"), "/f"])?;
        reg(&["delete", &format!(r"{shell}\{LEGACY_VERB}"), "/f"])?;
        if is_ours(&shell) {
            reg_ok(&["delete", &shell, "/ve", "/f"])?;
        }
    }
    for key in [NEW_WINDOW_KEY, OPEN_KEY].into_iter().chain(SHELL_OBJECTS.iter().map(|(key, _)| *key)) {
        if is_ours(&format!(r"{key}\command")) {
            reg_ok(&["delete", key, "/f"])?;
        }
    }
    Ok(())
}

// Whether the default value of `key` points at this app, so a value some
// other program set is left alone. The verb's name says "prime-files" ("nohrs"
// when a version up to 0.3.9 wrote it); a command names the executable, which
// the installer calls PrimeFiles.exe.
fn is_ours(key: &str) -> bool {
    let exe_name = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.file_name().map(|name| name.to_string_lossy().to_ascii_lowercase()));
    reg(&["query", key, "/ve"])
        .map(|output| {
            String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                let line = line.to_ascii_lowercase();
                line.contains("reg_sz")
                    && (line.contains(VERB) || line.contains(LEGACY_VERB) || exe_name.as_deref().is_some_and(|name| line.contains(name)))
            })
        })
        .unwrap_or(false)
}

// Where Windows starts programs at sign-in for this user, and the value name.
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Prime Files";

/// Whether Prime Files starts, minimized, when this user signs in.
pub fn starts_with_windows() -> bool {
    reg(&["query", RUN_KEY, "/v", RUN_VALUE]).is_ok_and(|output| output.status.success())
}

/// Makes `exe` start minimized at sign-in, or stops it. Started that way it
/// is already running when Premiere or a browser asks for a folder, so the
/// file opens here at once instead of in Explorer.
pub fn set_starts_with_windows(exe: &Path, enable: bool) -> io::Result<()> {
    if enable {
        let command = format!("\"{}\" --background", exe.display());
        reg_ok(&["add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", &command, "/f"])
    } else if starts_with_windows() {
        reg_ok(&["delete", RUN_KEY, "/v", RUN_VALUE, "/f"])
    } else {
        Ok(())
    }
}
