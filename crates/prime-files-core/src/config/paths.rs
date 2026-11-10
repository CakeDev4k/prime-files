//! XDG-style path resolution for Prime Files data locations.
//!
//! Per `docs/config.md` §1 we resolve `$XDG_CONFIG_HOME` / `$XDG_DATA_HOME` /
//! `$XDG_CACHE_HOME` directly, falling back to `~/.config`, `~/.local/share`,
//! and `~/.cache`. We deliberately do **not** use `dirs::config_dir()` and
//! friends: on macOS those return OS-native paths (`~/Library/...`) which
//! conflict with the dotfile layout Prime Files uses on every platform. `dirs` is
//! used only to locate the home directory for the fallback.

use std::path::PathBuf;

/// Application directory name used under each XDG base directory.
const APP_DIR: &str = "prime-files";

/// What that directory was called up to 0.3.9, when the program still carried
/// the name of the project it was forked from. People's settings, tags and
/// history are in there, so it is still looked for (see [`app_dir`]).
const LEGACY_APP_DIR: &str = "nohrs";

/// The application directory under `base`: the current name, or the old one
/// while that is the only one there.
///
/// [`adopt_legacy_dirs`] renames the old directory at startup, but a rename
/// fails while another copy of the program has files open in it. Reading the
/// old directory then is what keeps that person's settings on screen; starting
/// from an empty new one would look like everything was lost.
fn app_dir(base: PathBuf) -> PathBuf {
    let current = base.join(APP_DIR);
    if current.exists() {
        return current;
    }
    let legacy = base.join(LEGACY_APP_DIR);
    if legacy.exists() { legacy } else { current }
}

/// Renames the directories an older version left under the old name to the
/// current one. Meant to run once at startup, before anything opens a file in
/// them. A directory that cannot be renamed (in use by another copy) is left
/// where it is and keeps being read from there.
pub fn adopt_legacy_dirs() {
    let mut bases = vec![config_home(), data_home(), cache_home(), state_home()];
    bases.dedup();
    for base in bases {
        adopt(&base.join(LEGACY_APP_DIR), &base.join(APP_DIR));
    }
    if let Some(home) = dirs::home_dir() {
        adopt(&home.join(format!(".{LEGACY_APP_DIR}")), &home.join(format!(".{APP_DIR}")));
    }
}

/// Moves `legacy` to `current` unless `current` is already there: two
/// directories means a newer version already started fresh, and its data wins.
fn adopt(legacy: &std::path::Path, current: &std::path::Path) {
    if legacy.exists() && !current.exists() {
        // To stderr, not the log: the log lives in one of these directories,
        // so it cannot be open yet. The fallback in `app_dir` covers the
        // failure itself.
        if let Err(error) = std::fs::rename(legacy, current) {
            eprintln!("could not rename {} to {}: {error}", legacy.display(), current.display());
        }
    }
}

/// The dot directory in the home folder that holds the search index
/// (`~/.prime-files`), or the one an older version made while only that exists.
pub fn home_dot_dir() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let current = home.join(format!(".{APP_DIR}"));
    let legacy = home.join(format!(".{LEGACY_APP_DIR}"));
    Some(if !current.exists() && legacy.exists() { legacy } else { current })
}

/// Resolve an XDG base directory: use the environment variable if it is set to
/// an absolute path (relative values are ignored per the XDG spec), otherwise
/// fall back to `~/<default_subdir>`. Falls back to the current directory if the
/// home directory cannot be determined, so callers always get a usable path.
fn xdg_base(env_var: &str, default_subdir: &str) -> PathBuf {
    if let Some(value) = std::env::var_os(env_var) {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            return path;
        }
    }
    match dirs::home_dir() {
        Some(home) => home.join(default_subdir),
        None => PathBuf::from(default_subdir),
    }
}

/// `$XDG_CONFIG_HOME` or `~/.config`.
pub fn config_home() -> PathBuf {
    xdg_base("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_DATA_HOME` or `~/.local/share`.
pub fn data_home() -> PathBuf {
    xdg_base("XDG_DATA_HOME", ".local/share")
}

/// `$XDG_CACHE_HOME` or `~/.cache`.
pub fn cache_home() -> PathBuf {
    xdg_base("XDG_CACHE_HOME", ".cache")
}

/// `$XDG_STATE_HOME` or `~/.local/state`.
pub fn state_home() -> PathBuf {
    xdg_base("XDG_STATE_HOME", ".local/state")
}

/// The Prime Files config directory (`.../prime-files`).
pub fn config_dir() -> PathBuf {
    app_dir(config_home())
}

/// The Prime Files data directory (`.../prime-files`).
pub fn data_dir() -> PathBuf {
    app_dir(data_home())
}

/// The Prime Files cache directory (`.../prime-files`).
pub fn cache_dir() -> PathBuf {
    app_dir(cache_home())
}

/// The Prime Files state directory (`.../prime-files`).
///
/// State is what a user would keep across restarts but would not miss if it
/// were lost, and would not want backed up: logs go here rather than in
/// [`data_dir`] beside the databases, or [`cache_dir`], which a cleaner may
/// empty at any moment.
pub fn state_dir() -> PathBuf {
    app_dir(state_home())
}

/// Directory holding the rolling log files (`.../prime-files/logs`).
pub fn log_dir() -> PathBuf {
    state_dir().join("logs")
}

/// Full path to `config.toml`.
pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

#[cfg(test)]
// Rust 2024 made `std::env::{set_var, remove_var}` unsafe; these tests must
// mutate the process environment to exercise XDG path resolution.
#[allow(clippy::unwrap_used, unsafe_code)]
mod tests {
    use super::*;
    use crate::config::test_env::env_lock;

    const HOMES: [&str; 4] = ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"];

    /// Points the four base directories at an empty temporary folder for the
    /// length of a test. Without it the answer depends on the machine: one
    /// that still has a directory under the old name is read from there.
    struct EmptyHomes(tempfile::TempDir);

    impl EmptyHomes {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            for key in HOMES {
                unsafe { std::env::set_var(key, dir.path().join(key)) };
            }
            Self(dir)
        }

        fn base(&self, key: &str) -> PathBuf {
            self.0.path().join(key)
        }
    }

    impl Drop for EmptyHomes {
        fn drop(&mut self) {
            for key in HOMES {
                unsafe { std::env::remove_var(key) };
            }
        }
    }

    #[test]
    fn a_directory_under_the_old_name_is_still_read() {
        let _guard = env_lock();
        let homes = EmptyHomes::new();
        let legacy = homes.base("XDG_CONFIG_HOME").join("nohrs");
        std::fs::create_dir_all(&legacy).unwrap();
        assert_eq!(config_dir(), legacy);
        // The other three have neither, so they get the current name.
        assert!(data_dir().ends_with("prime-files"));
    }

    #[test]
    fn the_current_name_wins_over_the_old_one() {
        let _guard = env_lock();
        let homes = EmptyHomes::new();
        let base = homes.base("XDG_CONFIG_HOME");
        std::fs::create_dir_all(base.join("nohrs")).unwrap();
        std::fs::create_dir_all(base.join("prime-files")).unwrap();
        assert_eq!(config_dir(), base.join("prime-files"));
    }

    #[test]
    fn adopting_moves_the_old_directory_with_what_is_in_it() {
        let _guard = env_lock();
        let homes = EmptyHomes::new();
        let base = homes.base("XDG_CONFIG_HOME");
        std::fs::create_dir_all(base.join("nohrs")).unwrap();
        std::fs::write(base.join("nohrs").join("config.toml"), "kept").unwrap();
        adopt(&base.join("nohrs"), &base.join("prime-files"));
        assert!(!base.join("nohrs").exists());
        assert_eq!(std::fs::read_to_string(config_file()).unwrap(), "kept");
    }

    #[test]
    fn adopting_never_replaces_a_directory_that_is_already_there() {
        let _guard = env_lock();
        let homes = EmptyHomes::new();
        let base = homes.base("XDG_CONFIG_HOME");
        std::fs::create_dir_all(base.join("nohrs")).unwrap();
        std::fs::create_dir_all(base.join("prime-files")).unwrap();
        std::fs::write(base.join("prime-files").join("config.toml"), "newer").unwrap();
        adopt(&base.join("nohrs"), &base.join("prime-files"));
        assert!(base.join("nohrs").exists());
        assert_eq!(std::fs::read_to_string(config_file()).unwrap(), "newer");
    }

    #[test]
    fn config_file_lives_under_config_home() {
        // Reads `XDG_CONFIG_HOME`, so it must serialize with the mutating tests.
        let _guard = env_lock();
        let _homes = EmptyHomes::new();
        let file = config_file();
        assert!(file.ends_with("prime-files/config.toml"));
    }

    #[test]
    fn absolute_xdg_var_is_honoured() {
        let _guard = env_lock();
        // TODO: Audit that the environment access only happens in single-threaded code.
        // From `temp_dir` rather than a literal `/tmp/...`: on Windows that has no
        // drive, so it is not absolute and is rightly ignored.
        let absolute = std::env::temp_dir().join("xdg-test-abs");
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &absolute) };
        let dir = config_dir();
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        assert_eq!(dir, absolute.join("prime-files"));
    }

    #[test]
    fn relative_xdg_var_is_ignored() {
        let _guard = env_lock();
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "relative/path") };
        let home = config_home();
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        // A relative value is ignored per the XDG spec, so we fall back to a home
        // (or cwd) based path, never the relative value itself.
        assert!(!home.ends_with("relative/path"));
    }

    #[test]
    fn each_home_honours_its_own_xdg_var() {
        let _guard = env_lock();
        // Absolute on every platform; see `absolute_xdg_var_is_honoured`.
        let temp = std::env::temp_dir();
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", temp.join("cfg")) };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("XDG_DATA_HOME", temp.join("data")) };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("XDG_CACHE_HOME", temp.join("cache")) };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("XDG_STATE_HOME", temp.join("state")) };
        let (config, data, cache, state) = (config_home(), data_home(), cache_home(), state_home());
        for key in [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
        ] {
            // TODO: Audit that the environment access only happens in single-threaded code.
            unsafe { std::env::remove_var(key) };
        }
        assert_eq!(config, temp.join("cfg"));
        assert_eq!(data, temp.join("data"));
        assert_eq!(cache, temp.join("cache"));
        assert_eq!(state, temp.join("state"));
    }

    #[test]
    fn logs_live_under_the_state_directory() {
        // Not under `data_dir` (backed up beside the databases) and not under
        // `cache_dir` (a cleaner may empty it while Prime Files is running).
        let _guard = env_lock();
        let _homes = EmptyHomes::new();
        let logs = log_dir();
        assert!(logs.ends_with("prime-files/logs"), "{}", logs.display());
        assert!(logs.starts_with(state_home()), "{}", logs.display());
    }

    #[test]
    fn app_dirs_append_app_name_and_config_file_is_under_config_dir() {
        // Reads the XDG-derived dirs, so serialize with the mutating tests.
        let _guard = env_lock();
        let _homes = EmptyHomes::new();
        assert!(config_dir().ends_with("prime-files"));
        assert!(data_dir().ends_with("prime-files"));
        assert!(cache_dir().ends_with("prime-files"));
        assert_eq!(config_file(), config_dir().join("config.toml"));
    }
}
