//! Helpers shared by unit tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Characters in the name of each test folder that often cause errors: a space, an
/// apostrophe, a non-ASCII letter and `&` (HTML, URLs and command lines treat them
/// specially). User names such as `O'Brien` put them into `%TEMP%`, so every test
/// that uses files also checks them.
const DIFFICULT_CHARACTERS: &str = "'é&";

/// A temporary folder that is deleted when the value is dropped.
///
/// The folder is below the system temporary folder. Tests that examine parent folders
/// must stop their search at [`TempDir::path`] (for example with
/// `Context::with_ceiling` or `Context::load_in`), because the folders above it differ
/// from computer to computer.
pub struct TempDir(PathBuf);

impl TempDir {
    /// Makes a new, empty folder. `name` makes it easy to find if a test leaves it.
    pub fn new(name: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "mdviewer test {DIFFICULT_CHARACTERS} {name}-{}-{unique}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temporary test folder");
        Self(path)
    }

    /// The full path of the folder.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
