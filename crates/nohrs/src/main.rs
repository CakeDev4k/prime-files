//! The `nohrs` binary: parses CLI arguments, runs headless `config` subcommands
//! when requested, and otherwise launches the gpui desktop application.

mod app;
mod cli;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();

    // `config` subcommands run headless and exit without opening a window.
    if let Some(cli::Command::Config { action }) = &cli.command {
        nohrs_core::telemetry::logging::init_logging();
        std::process::exit(cli::run_config_command(action, &cli));
    }

    // One window per user: a running instance takes the path (or just comes
    // forward), which is what Listary and "Open with" expect of a file
    // manager. Only when nobody answers does this process open a window.
    if cli.command.is_none() {
        let handed_over = match &cli.path {
            Some(path) => nohrs_pages::ipc::request_open(path),
            None => nohrs_pages::ipc::request_show(),
        };
        if handed_over {
            return;
        }
    }

    app::NohrsApp::run(&cli);
}
