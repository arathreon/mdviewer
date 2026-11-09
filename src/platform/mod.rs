//! The code that is different on each operating system.
//!
//! All other modules are the same on all systems. They make the decisions (which link
//! opens, which request is blocked, which file a page can load), and they call this
//! module only to do the system part. Thus, the security rules are the same everywhere.
//!
//! Only Windows is done now. A build for another system stops with a clear message
//! (below). A port adds a folder next to `windows/` (for example `macos/`, `linux/`) that
//! gives the same items, and selects it here. The compiler then shows each missing item.
//!
//! Each platform module must give:
//!
//! | Item | What it does | Windows |
//! |---|---|---|
//! | `ORIGIN` | The origin of the page (`<scheme>://<host>`) for the `mdviewer` protocol. | `https://mdviewer.localhost` |
//! | `configure_webview` | Engine options, and the options that make the page use `ORIGIN`. | WebView2 browser arguments |
//! | `install_request_filter` | Sends each request of the web view through `network::block_reason`. Must fail if it cannot (fail closed). | `WebResourceRequested` |
//! | `pick_markdown_file` | The native "Open" dialog. `Ok(None)` when the user cancels. | `IFileOpenDialog` |
//! | `open_external` | Opens an `http`, `https` or `mailto` address in the default program. Checks the scheme again. | `ShellExecuteW` |
//! | `reveal` | Shows a file in the file manager, selected. Never runs it. | `SHOpenFolderAndSelectItems` |
//! | `work_area` | The screen without the taskbar or dock, in physical pixels. | `SPI_GETWORKAREA` |
//! | `config_base` | The base folder for settings (and Obsidian's `obsidian/obsidian.json`). | `%APPDATA%` |
//! | `local_data_base` | The base folder for data that stays on this computer (log, web view cache). | `%LOCALAPPDATA%` |
//! | `normal_form` | A resolved path (`fs::canonicalize`) in the form that users and other programs write. | removes `\\?\` |
//! | `same_name` | Whether two file or folder names are the same name for the file system. | letter case is ignored |
//!
//! The things that a port must also check (see `docs/architecture.md`, "Platforms"):
//! the origin check in `assets/viewer.js`, the keys (Cmd on macOS), and the tests that
//! use Windows paths.

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(windows))]
compile_error!(
    "mdviewer supports only Windows now. A port must add a module in src/platform \
     (see src/platform/mod.rs and docs/architecture.md, \"Platforms\")."
);
