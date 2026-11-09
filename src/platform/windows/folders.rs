//! The base folders of the user on Windows.
//!
//! These functions only read the environment. The `mdviewer` folder below them, and the
//! checks (absolute path, not empty), are in `paths.rs`, the same for all systems.

use std::ffi::OsString;

/// `%APPDATA%` (roaming): the settings of the user, and Obsidian's vault list.
pub fn config_base() -> Option<OsString> {
    std::env::var_os("APPDATA")
}

/// `%LOCALAPPDATA%`: data that stays on this computer (log, WebView2 cache).
pub fn local_data_base() -> Option<OsString> {
    std::env::var_os("LOCALAPPDATA")
}
