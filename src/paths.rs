//! Folders and files of the viewer.
//!
//! The base folders come from the system ([`platform::config_base`],
//! [`platform::local_data_base`]). The checks and the names below them are here, the
//! same for all systems.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::platform;

/// The name of the file with the dialect choices for each folder (see
/// `dialect::choices`). It is next to `settings.ini`.
const DIALECTS_FILE_NAME: &str = "dialects.ini";

/// Gives the `mdviewer` folder below a base folder, for example below
/// `%LOCALAPPDATA%` or `%APPDATA%` on Windows.
///
/// Returns `None` if the base folder is missing, empty or not absolute, so that the
/// viewer never writes into the current working folder.
pub fn app_dir(base: Option<OsString>) -> Option<PathBuf> {
    let base = PathBuf::from(base?);
    base.is_absolute().then(|| base.join("mdviewer"))
}

/// `<local data base>\mdviewer` (Windows: `%LOCALAPPDATA%\mdviewer`): data that stays on
/// this computer (logs, web view cache).
pub fn local_dir_from_env() -> Option<PathBuf> {
    app_dir(platform::local_data_base())
}

/// The folder for the WebView2 data (cache, settings).
pub fn webview_data_dir(local_dir: PathBuf) -> PathBuf {
    local_dir.join("WebView2")
}

/// The settings file below a base folder: `<base>\mdviewer\settings.ini`.
pub fn settings_file(app_data: Option<OsString>) -> Option<PathBuf> {
    app_dir(app_data).map(|dir| dir.join("settings.ini"))
}

/// `<config base>\mdviewer\settings.ini` (Windows: `%APPDATA%\mdviewer\settings.ini`):
/// the settings of the user.
pub fn settings_file_from_env() -> Option<PathBuf> {
    settings_file(platform::config_base())
}

/// `<config base>\mdviewer\dialects.ini` (Windows: `%APPDATA%\mdviewer\dialects.ini`):
/// the dialect that the user chose for each folder.
pub fn dialects_file_from_env() -> Option<PathBuf> {
    app_dir(platform::config_base()).map(|dir| dir.join(DIALECTS_FILE_NAME))
}

/// Gives `true` if `path` is `root` or below it. The paths are compared part by part,
/// so `D:\Notes2` is not below `D:\Notes`. Each name is compared with the rules of the
/// file system ([`platform::same_name`]; on Windows, letter case is ignored).
pub fn is_inside(path: &Path, root: &Path) -> bool {
    let mut parts = path.components().filter(|part| *part != Component::CurDir);
    root.components()
        .filter(|part| *part != Component::CurDir)
        .all(|root_part| {
            parts
                .next()
                .is_some_and(|part| platform::same_name(part.as_os_str(), root_part.as_os_str()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_inside_compares_whole_names() {
        let root = Path::new(r"D:\Notes");
        assert!(is_inside(Path::new(r"D:\Notes"), root));
        assert!(is_inside(Path::new(r"D:\Notes\a.md"), root));
        assert!(is_inside(Path::new(r"D:\Notes\Daily\a.md"), root));
        assert!(!is_inside(Path::new(r"D:\Notes2\a.md"), root));
        assert!(!is_inside(Path::new(r"D:\"), root));
        assert!(!is_inside(Path::new(r"E:\Notes\a.md"), root));
        // A trailing separator does not matter.
        assert!(is_inside(
            Path::new(r"D:\Notes\a.md"),
            Path::new(r"D:\Notes\")
        ));
    }

    #[test]
    #[cfg(windows)]
    fn is_inside_ignores_letter_case_on_windows() {
        assert!(is_inside(
            Path::new(r"d:\NOTES\daily\a.md"),
            Path::new(r"D:\Notes\Daily")
        ));
    }

    #[test]
    fn dialects_file_is_next_to_the_settings() {
        let settings = settings_file_from_env();
        let dialects = dialects_file_from_env();
        assert_eq!(
            settings.as_deref().and_then(Path::parent),
            dialects.as_deref().and_then(Path::parent)
        );
        if let Some(dialects) = dialects {
            assert!(dialects.ends_with("mdviewer/dialects.ini"));
        }
    }

    #[test]
    fn app_dir_is_below_base() {
        let base = std::env::temp_dir();
        assert_eq!(
            app_dir(Some(base.clone().into_os_string())),
            Some(base.join("mdviewer"))
        );
    }

    #[test]
    fn app_dir_rejects_missing_empty_and_relative_base() {
        assert_eq!(app_dir(None), None);
        assert_eq!(app_dir(Some(OsString::new())), None);
        assert_eq!(app_dir(Some(OsString::from("relative\\folder"))), None);
    }

    #[test]
    fn webview_data_dir_is_below_local_dir() {
        let base = std::env::temp_dir().join("mdviewer");
        assert_eq!(webview_data_dir(base.clone()), base.join("WebView2"));
    }

    #[test]
    fn settings_file_is_below_app_dir() {
        let base = std::env::temp_dir();
        assert_eq!(
            settings_file(Some(base.clone().into_os_string())),
            Some(base.join("mdviewer").join("settings.ini"))
        );
        assert_eq!(settings_file(Some(OsString::from("relative"))), None);
    }
}
