//! Windows file-name rules.

use std::ffi::OsStr;
use std::path::PathBuf;

/// Changes a Windows "verbatim" path from `fs::canonicalize` into the normal form:
/// `\\?\C:\x` → `C:\x` and `\\?\UNC\server\share\x` → `\\server\share\x`. Other paths
/// do not change. Without this, comparisons with normal paths (for example the vault
/// list of Obsidian) fail, and logs show the prefix.
pub fn normal_form(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

/// Gives `true` if two names (or two whole paths) are the same for the Windows file
/// system: letter case is ignored, so `Note.md` and `note.MD` are one file.
pub fn same_name(a: &OsStr, b: &OsStr) -> bool {
    a == b || a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_paths_become_normal_paths() {
        assert_eq!(
            normal_form(PathBuf::from(r"\\?\C:\a\b.md")),
            PathBuf::from(r"C:\a\b.md")
        );
        assert_eq!(
            normal_form(PathBuf::from(r"\\?\UNC\server\share\x")),
            PathBuf::from(r"\\server\share\x")
        );
        assert_eq!(normal_form(PathBuf::from(r"C:\a")), PathBuf::from(r"C:\a"));
        // Other verbatim forms (for example volume GUIDs) stay as they are.
        let volume = r"\\?\Volume{12345678-0000-0000-0000-000000000000}\x";
        assert_eq!(normal_form(PathBuf::from(volume)), PathBuf::from(volume));
    }

    #[test]
    fn names_ignore_letter_case_only() {
        let name = |text: &str| OsStr::new(text).to_owned();
        assert!(same_name(&name("Note.md"), &name("note.MD")));
        assert!(same_name(&name("Čeština"), &name("čEŠTINA")));
        assert!(!same_name(&name("a.md"), &name("b.md")));
        assert!(!same_name(&name("a.md"), &name("a.md ")));
    }
}
