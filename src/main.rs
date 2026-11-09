//! The `mdviewer` program: `mdviewer [file.md]`.
//!
//! It starts the log, reads the settings, makes the first page and opens the window.
//! All other code is in the library (`src/lib.rs`), so that tests can use it.

// Release builds are window programs without a console. Debug builds keep the
// console, so that `cargo run` shows errors directly.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::Path;
use std::time::Instant;

use mdviewer::dialect::choices::FolderChoices;
use mdviewer::page::{APP_NAME, Page};
use mdviewer::settings::Settings;
use mdviewer::{paths, viewer};

fn main() {
    let started = Instant::now();
    let log_file = mdviewer::logging::init();
    if cfg!(debug_assertions) {
        match &log_file {
            Some(path) => eprintln!("log file: {}", path.display()),
            None => eprintln!("logging is off or not available"),
        }
    }

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    log::info!(
        "mdviewer {} started, args: {args:?}",
        env!("CARGO_PKG_VERSION")
    );

    // The dialect that the user chose for each folder ("Dialect" in the toolbar).
    let dialects_path = paths::dialects_file_from_env();
    let dialects = dialects_path
        .as_deref()
        .map(FolderChoices::load)
        .unwrap_or_default();
    log::debug!("dialect choices for {} folders", dialects.len());

    let page = match args.first() {
        Some(path) => Page::from_file(Path::new(path), &dialects),
        None => {
            log::warn!("no file given");
            Page::message(
                APP_NAME,
                "No file",
                "Click \"Open…\" at the top right or press Ctrl+O to open a Markdown file.\n\
                 You can also open .md files with mdviewer from File Explorer.",
            )
        }
    };

    let settings_path = paths::settings_file_from_env();
    let settings = settings_path
        .as_deref()
        .map(Settings::load)
        .unwrap_or_default();
    log::debug!("settings: {settings:?}");

    if let Err(error) = viewer::run(
        page,
        viewer::Saved {
            settings,
            settings_path,
            dialects,
            dialects_path,
        },
        started,
    ) {
        // For example, the WebView2 Runtime is not installed.
        log::error!("cannot show the window: {error}");
        std::process::exit(1);
    }

    log::info!("mdviewer finished");
}
