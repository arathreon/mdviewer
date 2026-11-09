//! The Windows platform: WebView2, the Windows Shell and the Windows file system.
//!
//! All `unsafe` code of the viewer is in this folder (`dialog.rs`, `shell.rs`,
//! `request_filter.rs`). The list of items that this module gives is in
//! `src/platform/mod.rs`.

mod dialog;
mod files;
mod folders;
mod request_filter;
mod shell;
mod webview;

pub use dialog::pick_markdown_file;
pub use files::{normal_form, same_name};
pub use folders::{config_base, local_data_base};
pub use request_filter::install as install_request_filter;
pub use shell::{open_external, reveal, work_area};
pub use webview::{ORIGIN, configure_webview};
