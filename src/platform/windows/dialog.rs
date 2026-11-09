//! The Windows "Open" dialog (`IFileOpenDialog`).
//!
//! The dialog is native, not an HTML `<input type="file">`: the page would get only the
//! file content, but the viewer needs the path (dialect detection, relative images).

use std::path::{Path, PathBuf};

use tao::platform::windows::WindowExtWindows;
use tao::window::Window;
use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
use windows::Win32::UI::Shell::{
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FileOpenDialog, IFileOpenDialog, IShellItem,
    SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::core::{HRESULT, HSTRING, PCWSTR};

use crate::page::MARKDOWN_EXTENSIONS;

/// Shows the dialog and gives the selected file, or `None` if the user cancels.
///
/// The dialog is modal to `owner`. `folder` is the folder that the dialog
/// shows first (if it does not exist, Windows uses its own choice).
pub fn pick_markdown_file(
    owner: &Window,
    folder: Option<&Path>,
) -> windows::core::Result<Option<PathBuf>> {
    let markdown = HSTRING::from(filter_spec(MARKDOWN_EXTENSIONS));
    let all = HSTRING::from("*.*");
    let markdown_name = HSTRING::from("Markdown files");
    let all_name = HSTRING::from("All files");
    let filters = [
        COMDLG_FILTERSPEC {
            pszName: PCWSTR(markdown_name.as_ptr()),
            pszSpec: PCWSTR(markdown.as_ptr()),
        },
        COMDLG_FILTERSPEC {
            pszName: PCWSTR(all_name.as_ptr()),
            pszSpec: PCWSTR(all.as_ptr()),
        },
    ];

    // SAFETY: COM is initialized on this (the UI) thread by tao/wry. All strings live
    // until the end of this function, after the dialog closes.
    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetTitle(&HSTRING::from("Open Markdown file"))?;
        dialog.SetFileTypes(&filters)?;
        dialog.SetOptions(dialog.GetOptions()? | FOS_FILEMUSTEXIST | FOS_FORCEFILESYSTEM)?;
        if let Some(folder) = folder {
            let folder = HSTRING::from(folder.as_os_str());
            // A missing folder is not an error: the dialog then uses its last folder.
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&folder, None) {
                let _ = dialog.SetFolder(&item);
            }
        }

        if let Err(error) = dialog.Show(Some(HWND(owner.hwnd() as _))) {
            return if is_cancel(error.code()) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let name = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let path = name.to_hstring().to_os_string();
        CoTaskMemFree(Some(name.0 as _));
        Ok(Some(PathBuf::from(path)))
    }
}

/// The filter text of the dialog, for example `"*.md;*.markdown"`.
pub fn filter_spec(extensions: &[&str]) -> String {
    extensions
        .iter()
        .map(|extension| format!("*.{extension}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// The dialog gives this error code when the user closes it without a file.
fn is_cancel(code: HRESULT) -> bool {
    code == ERROR_CANCELLED.to_hresult()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_spec_lists_all_extensions() {
        assert_eq!(filter_spec(&["md", "markdown"]), "*.md;*.markdown");
        assert_eq!(filter_spec(&[]), "");
        assert!(filter_spec(MARKDOWN_EXTENSIONS).starts_with("*.md;*.markdown;"));
    }

    #[test]
    fn only_the_cancel_code_means_cancel() {
        // HRESULT_FROM_WIN32(ERROR_CANCELLED)
        assert!(is_cancel(HRESULT(0x8007_04C7_u32 as i32)));
        assert!(!is_cancel(HRESULT(0)));
        // E_ACCESSDENIED
        assert!(!is_cancel(HRESULT(0x8007_0005_u32 as i32)));
    }
}
