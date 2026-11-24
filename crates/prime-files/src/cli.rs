//! Command-line interface: argument parsing plus the `prime-files config` subcommands
//! (config.md §8). With no subcommand the parsed global overrides are handed to
//! the GUI; with a `config` subcommand we run it and exit without opening a
//! window.

use clap::{Parser, Subcommand};
use prime_files_core::config::{self, ConfigOverride, ThemeMode};
use std::process::Command as ProcessCommand;

#[derive(Parser, Debug)]
#[command(
    name = "prime-files",
    version,
    about = "Launcher × Explorer file workspace",
    // A path and a subcommand are two different invocations; without this a
    // path named like a subcommand would be ambiguous.
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    /// Override the theme mode for this run: system, light, or dark.
    #[arg(long, global = true, value_name = "MODE")]
    pub theme: Option<String>,

    /// Override the accent colour for this run (named colour or hex).
    #[arg(long, global = true, value_name = "COLOR")]
    pub accent: Option<String>,

    /// Start minimized, as at sign-in (Settings › Windows integration › Start
    /// with Windows). A running instance is left as it is.
    #[arg(long)]
    pub background: bool,

    /// Runs only the watch that opens the Explorer's folder windows here
    /// while the app is closed (issue #60). Started by the app and at
    /// sign-in, with Prime Files the default explorer.
    #[arg(long, hide = true)]
    pub watch_explorer: bool,

    /// Ends a running `--watch-explorer` (the uninstaller).
    #[arg(long, hide = true)]
    pub stop_watching: bool,

    /// A folder to open, or a file to open its folder with it selected. When
    /// Prime Files is already running it opens there, in a new tab.
    #[arg(value_name = "PATH", value_parser = parse_path)]
    pub path: Option<std::path::PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Inspect or manage the user configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Open the launcher window on its own, without the explorer.
    Launcher,
    /// Gives Listary's folders back to Explorer if they open here. Run by the
    /// uninstaller, which cannot edit Listary's JSON itself.
    #[command(hide = true)]
    ForgetListary,
    /// Records the language chosen in the interactive installer in
    /// `config.toml` (creating it from defaults if missing, or updating only
    /// `ui.language` while preserving all other settings; silent updates skip
    /// it).
    #[command(hide = true)]
    InitLanguage {
        /// Language code (e.g. `es`, `pt-BR`, `en`).
        language: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Print the fully merged config (defaults < file < env < CLI) as JSON.
    Show,
    /// Open config.toml in $EDITOR (creating it with defaults if missing).
    Edit,
    /// Validate config.toml, reporting parse errors and warnings.
    Validate,
    /// Print the JSON Schema for config.toml to stdout.
    Schema,
    /// Print the full path to config.toml.
    Path,
    /// Reset config.toml to defaults, backing up any existing file first.
    Reset,
}

impl Cli {
    /// Build the CLI override layer from the global `--theme` / `--accent` flags.
    /// An invalid `--theme` value is warned about and ignored rather than
    /// aborting (it still leaves the file/env value in effect).
    pub fn overrides(&self) -> ConfigOverride {
        let mut over = ConfigOverride::default();
        if let Some(theme) = &self.theme {
            match ThemeMode::parse(theme.trim()) {
                Some(mode) => over.theme_mode = Some(mode),
                None => tracing::warn!("ignoring invalid --theme {theme:?}"),
            }
        }
        if let Some(accent) = &self.accent {
            over.theme_accent = Some(accent.clone());
        }
        over
    }
}

/// Run a `config` subcommand. Returns the process exit code.
pub fn run_config_command(action: &ConfigAction, cli: &Cli) -> i32 {
    let path = config::paths::config_file();
    match action {
        ConfigAction::Path => {
            println!("{}", path.display());
            0
        }
        ConfigAction::Schema => match config::json_schema_string() {
            Ok(schema) => {
                println!("{schema}");
                0
            }
            Err(error) => {
                eprintln!("failed to generate schema: {error}");
                1
            }
        },
        ConfigAction::Show => {
            let (mut merged, diagnostics) = config::load_from_path(&path);
            config::report_diagnostics(&diagnostics);
            merged.apply_override(&ConfigOverride::from_env());
            merged.apply_override(&cli.overrides());
            match merged.to_json() {
                Ok(json) => {
                    println!("{json}");
                    0
                }
                Err(error) => {
                    eprintln!("failed to serialize config: {error}");
                    1
                }
            }
        }
        ConfigAction::Validate => {
            let (_, diagnostics) = config::load_from_path(&path);
            let error = config::report_diagnostics(&diagnostics);
            if let Some(message) = error {
                eprintln!("invalid config ({}): {message}", path.display());
                1
            } else {
                let warnings = diagnostics.len();
                if warnings == 0 {
                    println!("{}: OK", path.display());
                } else {
                    println!(
                        "{}: OK with {warnings} warning(s) (see logs)",
                        path.display()
                    );
                }
                0
            }
        }
        ConfigAction::Reset => match config::reset(&path) {
            Ok(Some(backup)) => {
                println!(
                    "reset {}; backed up to {}",
                    path.display(),
                    backup.display()
                );
                0
            }
            Ok(None) => {
                println!("wrote default config to {}", path.display());
                0
            }
            Err(error) => {
                eprintln!("failed to reset config: {error}");
                1
            }
        },
        ConfigAction::Edit => edit_config(&path),
    }
}

/// Records the language chosen in the interactive installer in `config.toml`,
/// creating it from defaults if missing or updating only `ui.language` while
/// preserving all other settings. Returns the process exit code.
pub fn run_init_language(language: &str) -> i32 {
    let path = config::paths::config_file();
    match init_language_at(&path, language) {
        Ok(()) => 0,
        Err(error) => {
            tracing::error!("could not initialize language in {}: {error}", path.display());
            eprintln!("failed to initialize language: {error}");
            1
        }
    }
}

fn init_language_at(path: &std::path::Path, language: &str) -> Result<(), String> {
    let lang = prime_files_core::i18n::Lang::parse(language)
        .ok_or_else(|| format!("unknown language {language:?}"))?;
    config::set_value(path, "ui", "language", &format!("\"{}\"", lang.code()))
        .map_err(|error| error.to_string())
}

fn edit_config(path: &std::path::Path) -> i32 {
    if let Err(error) = config::ensure_exists(path) {
        eprintln!("failed to create {}: {error}", path.display());
        return 1;
    }
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());
    // `$EDITOR` often carries arguments (e.g. `code --wait`), so split on
    // whitespace rather than treating the whole string as a program path.
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    match ProcessCommand::new(program).args(parts).arg(path).status() {
        // A signal-terminated child reports no exit code; surface that as a
        // failure rather than masking it as success.
        Ok(status) if status.success() => 0,
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("failed to launch editor {editor:?}: {error}");
            1
        }
    }
}

/// Reads the `PATH` argument, repairing a quoted drive root.
///
/// Windows hands a folder over as `prime-files.exe "%1"`, and for a drive that is
/// `"C:\"`: by the rules every program parses its command line with, the
/// backslash escapes the closing quote, and the argument arrives as `C:"`.
/// A quote can never be part of a Windows path, so a trailing one is that
/// backslash.
fn parse_path(argument: &str) -> Result<std::path::PathBuf, std::convert::Infallible> {
    Ok(match argument.strip_suffix('"') {
        Some(root) => std::path::PathBuf::from(format!("{root}\\")),
        None => std::path::PathBuf::from(argument),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_dir(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("prime-files-cli-{name}-{}-{nanos}", std::process::id()))
    }

    #[test]
    fn a_drive_root_whose_quote_was_escaped_is_repaired() {
        let cli = Cli::parse_from(["prime-files", "C:\""]);
        assert_eq!(cli.path, Some(std::path::PathBuf::from("C:\\")));
        let cli = Cli::parse_from(["prime-files", r"C:\Users"]);
        assert_eq!(cli.path, Some(std::path::PathBuf::from(r"C:\Users")));
    }

    #[test]
    fn init_language_writes_and_updates_ui_language_preserving_other_settings() {
        let dir = unique_test_dir("fresh");
        let path = dir.join("config.toml");

        assert_eq!(init_language_at(&path, "es"), Ok(()));
        let (loaded, diagnostics) = config::load_from_path(&path);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(loaded.ui.language, "es");

        config::set_value(&path, "theme", "mode", "\"light\"").expect("set theme.mode");
        assert_eq!(init_language_at(&path, "pt-BR"), Ok(()));
        let (loaded, diagnostics) = config::load_from_path(&path);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(loaded.ui.language, "pt-BR");
        assert_eq!(loaded.theme.mode, ThemeMode::Light);

        // A commented `[ui]` header reuses the same table instead of duplicating it.
        std::fs::write(&path, "[ui] # interface settings\nshow_hidden = true\n").expect("write commented header");
        assert_eq!(init_language_at(&path, "es"), Ok(()));
        let raw = std::fs::read_to_string(&path).expect("read config");
        assert_eq!(raw.matches("[ui]").count(), 1, "unexpected duplicate [ui] in:\n{raw}");
        let (loaded, diagnostics) = config::load_from_path(&path);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(loaded.ui.language, "es");
        assert!(loaded.ui.show_hidden);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn init_language_rejects_unknown_language_without_creating_file() {
        let dir = unique_test_dir("unknown");
        let path = dir.join("config.toml");

        assert!(init_language_at(&path, "xx").is_err());
        assert!(!path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
