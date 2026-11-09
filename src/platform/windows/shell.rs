//! Windows Shell calls: open a web address in the default program, and show a file in
//! File Explorer. The decisions are in `links.rs` (tested); this file only calls Windows.
//! It also reads the work area of the screen, for the window position.

use std::path::Path;

use windows::Win32::Foundation::{E_FAIL, RECT};
use windows::Win32::UI::Shell::{
    ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETWORKAREA, SW_SHOWNORMAL, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::{HSTRING, w};

use crate::links::EXTERNAL_SCHEMES;

/// Opens a web or mail address in the default program.
///
/// The scheme is checked again here, so that a mistake in another file cannot start a
/// dangerous protocol handler.
pub fn open_external(address: &str) -> Result<(), String> {
    let scheme = address
        .split_once(':')
        .map(|(scheme, _)| scheme.to_ascii_lowercase());
    if !scheme.is_some_and(|scheme| EXTERNAL_SCHEMES.contains(&scheme.as_str())) {
        return Err(format!("not an allowed address: {address}"));
    }
    // SAFETY: all strings live until the call returns.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(address),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute gives a value above 32 on success.
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err(format!(
            "ShellExecute failed with code {}",
            result.0 as isize
        ))
    }
}

/// Opens File Explorer with the file (or folder) selected. Nothing is run.
pub fn reveal(path: &Path) -> windows::core::Result<()> {
    // SAFETY: the item list is freed after use; COM is initialized on the UI thread.
    unsafe {
        let item = ILCreateFromPathW(&HSTRING::from(path.as_os_str()));
        if item.is_null() {
            return Err(windows::core::Error::new(
                E_FAIL,
                format!("no shell item for {}", path.display()),
            ));
        }
        let result = SHOpenFolderAndSelectItems(item, None, 0);
        ILFree(Some(item));
        result
    }
}

/// The work area of the primary screen in physical pixels: the screen without the
/// taskbar. Gives `(left, top, right, bottom)`, or `None` if Windows does not give it.
pub fn work_area() -> Option<(i32, i32, i32, i32)> {
    let mut area = RECT::default();
    // SAFETY: `area` is a valid RECT and lives until the call returns.
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut area).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .ok()?;
    Some((area.left, area.top, area.right, area.bottom))
}
