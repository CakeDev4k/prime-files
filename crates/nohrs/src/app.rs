//! Application entry / startup sequence.
//!
//! The binary is the only layer allowed to depend on every crate, so it wires
//! the pillars together: it builds the shared window chrome from `nohrs-ui`,
//! initializes services that need the async runtime, and opens the Explorer
//! window hosting `nohrs_pages::RootView` and the launcher window hosting
//! `nohrs_launcher::LauncherView`. Neither pillar references the other; the
//! binary owns the keybinding that summons the launcher from anywhere.

use crate::cli::{Cli, Command};
use gpui::{App, AppContext, Application, KeyBinding, WindowAppearance};
use nohrs_core::config::{self, ConfigOverride, ThemeMode};
use nohrs_core::telemetry::logging::{FileLogConfig, init_logging_with_file};
use nohrs_launcher::{LauncherIndex, ToggleLauncher};
use nohrs_services::fs::trash;
use nohrs_services::search::SearchService;
use nohrs_store::{KvStore, RedbKvStore, StoreLogConfig};
use nohrs_ui::assets::Assets;
use std::sync::Arc;

pub struct NohrsApp;

impl NohrsApp {
    pub fn run(cli: &Cli) {
        // Held for the whole run: dropping it stops the log file being written.
        // Installed before the config is read so a failure to load the config is
        // itself recorded, which means the file sink uses its defaults rather
        // than anything the user set — see `docs/logging.md` §4.
        let _log_guard = init_logging_with_file(&FileLogConfig::default());
        nohrs_pages::startup::logging_ready();
        nohrs_pages::startup::mark("logging");

        // Load configuration before opening the window: defaults < file < env <
        // CLI (config.md §3). A missing file is created with defaults so users
        // have something to edit and the watcher has a target. Parse failures are
        // non-fatal — we fall back to defaults and surface the error in the UI.
        let config_path = config::paths::config_file();
        if let Err(error) = config::ensure_exists(&config_path) {
            tracing::warn!("could not create {}: {error}", config_path.display());
        }
        // Read here for what comes before any window (language, theme,
        // shortcuts); each window reads it again and reports its problems.
        let (mut config, _diagnostics) = config::load_from_path(&config_path);
        let config_overrides = vec![ConfigOverride::from_env(), cli.overrides()];
        for over in &config_overrides {
            config.apply_override(over);
        }

        // Before any window draws a word: "auto" needs to know the answer.
        nohrs_pages::locale::detect_and_record();
        nohrs_core::i18n::apply_preference(&config.ui.language);
        nohrs_core::shortcuts::apply(&config.keybindings.bindings);
        nohrs_pages::startup::mark("config");

        // GPUI drives the app (replacing `#[tokio::main]`; ADR 0004, async-runtime.md
        // §7). The search service is now tokio-free — its file watcher and progress
        // channels run on std threads and runtime-agnostic channels — so no async
        // runtime needs to be entered here.
        let launcher_only = matches!(cli.command, Some(Command::Launcher));
        let initial_path = cli.path.clone();
        let background = cli.background;

        Application::new().with_assets(Assets).run(move |app: &mut App| {
            // Direct3D and DirectWrite are up by now (gpui's platform).
            nohrs_pages::startup::mark("gpui");
            gpui_component::init(app);
            apply_theme_mode(config.theme.mode, app);

            // The name index is built once for the whole run and shared by every
            // launcher window, so summoning the launcher never waits on a scan.
            if launcher_only {
                let launcher = LauncherIndex::start(app);
                // Two ways in (docs/launcher.md §2): a chord the OS routes to
                // nohrs from any application, and one GPUI handles while nohrs
                // is in front. Only here: with the explorer, the quick search
                // (`nohrs_pages::quick_search`) answers both, as Prime Files'
                // own Listary, and the main window installs it.
                launcher.install_global_hotkey(app);
                install_in_app_launcher_key(launcher.clone(), app);
                // Resident mode: nohrs is a background process whose only job is
                // to answer the global hotkey, so it must outlive each dismissal
                // of the launcher window.
                if let Err(error) = nohrs_launcher::open_keep_alive_window(app) {
                    tracing::error!("failed to open the keep-alive window: {error}");
                }
                if let Err(error) = launcher.toggle(app) {
                    tracing::error!("failed to open launcher window: {error}");
                }
                return;
            }

            // Open the host KV store (`state.redb`) for tab/session restore
            // (docs/persistence.md §3). Failure is non-fatal: the app starts
            // without session persistence rather than crashing.
            let store: Option<Arc<dyn KvStore>> = open_host_store();

            // The trash ledger, on the platforms whose OS trash records nothing
            // (macOS). Without it the explorer's Delete is a one-way door: the
            // item is in `~/.Trash` but nothing knows where it came from, so
            // neither `noh trash restore` nor a future in-app restore can put it
            // back. Failing to open it is not fatal to the app and does not
            // become "no ledger needed" either — `Ledger` carries the difference
            // so Delete can refuse rather than silently lose the way back.
            let trash_ledger = trash::Ledger::open();

            // Initialize SearchService once, for every window. Failure is
            // non-fatal: the app starts with full-text search disabled.
            let search_service: Option<Arc<SearchService>> = match SearchService::new() {
                Ok(service) => Some(Arc::new(service)),
                Err(e) => {
                    tracing::error!("Failed to initialize search service; starting with search disabled: {}", e);
                    None
                }
            };
            // Initial indexing on GPUI's background executor, a thread pool
            // (replacing tokio::task::spawn_blocking; async-runtime.md §2).
            if let Some(service) = &search_service
                && let Some(job) = service.take_initial_indexing_job()
            {
                app.background_spawn(async move { job.run() }).detach();
            }

            // What every explorer window is built from: the first one is the
            // main window, and a tab dragged out opens another
            // (`nohrs_pages::windows`).
            app.set_global(nohrs_pages::windows::WindowSetup {
                search_service,
                store,
                trash_ledger,
                config_path: config_path.clone(),
                config_overrides: config_overrides.clone(),
            });
            nohrs_pages::startup::mark("services");
            let opened = nohrs_pages::windows::open_window(
                nohrs_pages::windows::WindowRequest { path: initial_path.clone(), bounds: None, background },
                app,
            );
            if let Err(error) = opened {
                tracing::error!("failed to open main window: {error}");
            }
            nohrs_pages::startup::mark("window");
        });
    }
}

/// Binds the launcher's in-app summon key and handles it at application level.
///
/// This is the half of docs/launcher.md §2 that GPUI can serve: it only fires
/// while a nohrs window has focus. Summoning the launcher over *other*
/// applications is the OS-level registration in `nohrs_launcher::hotkey`.
///
/// The handler is global rather than attached to a window so the same key works
/// from the explorer, from a launcher window (where it toggles the launcher
/// closed), and from any window added later — without either pillar having to
/// know the other exists.
///
/// `cmd-k` is the platform key on macOS and `ctrl-k` its equivalent elsewhere;
/// both are bound so the binding is right on either platform.
fn install_in_app_launcher_key(launcher: LauncherIndex, app: &mut App) {
    app.bind_keys([
        KeyBinding::new("cmd-k", ToggleLauncher, None),
        KeyBinding::new("ctrl-k", ToggleLauncher, None),
    ]);
    app.on_action(move |_: &ToggleLauncher, cx: &mut App| {
        if let Err(error) = launcher.toggle(cx) {
            tracing::error!("failed to toggle launcher window: {error}");
        }
    });
}

/// Opens the host KV store at `<data_dir>/state.redb`, creating the data
/// directory if needed. Returns `None` (and logs) on any failure so a broken or
/// unwritable store degrades to "no session persistence" rather than a crash.
fn open_host_store() -> Option<Arc<dyn KvStore>> {
    let data_dir = config::paths::data_dir();
    if let Err(error) = std::fs::create_dir_all(&data_dir) {
        tracing::error!("could not create data dir {}: {error}", data_dir.display());
        return None;
    }
    let path = data_dir.join("state.redb");
    match RedbKvStore::open(&path, &StoreLogConfig::default()) {
        Ok(store) => Some(Arc::new(store)),
        Err(error) => {
            tracing::error!("could not open KV store {}: {error}", path.display());
            None
        }
    }
}

/// Resolves `theme.mode` to light or dark and applies it to both palettes the
/// UI draws from: nohrs's own tokens and gpui-component's widgets.
///
/// The Explorer window does this again, and on every config reload, in
/// `RootView::apply_config`. This copy is for the launcher-only mode, which
/// never builds that root and would otherwise always open light.
fn apply_theme_mode(mode: ThemeMode, app: &mut App) {
    let dark = match mode {
        ThemeMode::Dark => true,
        ThemeMode::Light => false,
        ThemeMode::System => matches!(
            app.window_appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ),
    };
    nohrs_ui::theme::theme::set_dark(dark);
    let widget_mode = if dark {
        gpui_component::ThemeMode::Dark
    } else {
        gpui_component::ThemeMode::Light
    };
    gpui_component::Theme::change(widget_mode, None, app);
}
