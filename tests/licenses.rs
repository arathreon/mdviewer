//! Checks that the license files that a release must contain are in the repository.
//!
//! The release zip file (`.github/scripts/package-release.ps1`) copies them, and the
//! licenses of the embedded libraries require them. A library update can easily lose a
//! file, so these tests fail when one is missing.

use std::fs;
use std::path::{Path, PathBuf};

/// The root folder of the repository.
fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Reads a text file below the root, or fails with its name. CR LF becomes LF, because
/// Git on Windows can check out text files with CR LF.
fn read(relative: &str) -> String {
    fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("{relative}: {error}"))
        .replace("\r\n", "\n")
}

/// The library folders in `assets/vendor` (each embedded library has one).
fn vendor_folders() -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = fs::read_dir(root().join("assets/vendor"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    folders
}

#[test]
fn the_project_license_is_mit() {
    let license = read("LICENSE");
    assert!(license.starts_with("MIT License\n"), "{license}");
    assert!(license.contains("Copyright (c) "), "{license}");
    let manifest = read("Cargo.toml");
    assert!(
        manifest.contains("\nlicense = \"MIT\"\n"),
        "Cargo.toml has no MIT license field"
    );
}

#[test]
fn every_vendor_library_has_its_license() {
    let folders = vendor_folders();
    assert!(folders.len() >= 5, "{folders:?}");
    for folder in folders {
        let license = folder.join("LICENSE");
        let text = fs::read_to_string(&license)
            .unwrap_or_else(|error| panic!("{}: {error}", license.display()));
        assert!(text.contains("Copyright"), "{}", license.display());
    }
}

#[test]
fn the_katex_fonts_have_the_open_font_license() {
    let ofl = read("assets/vendor/katex/OFL.txt");
    assert!(
        ofl.contains("SIL OPEN FONT LICENSE Version 1.1"),
        "no OFL text"
    );
    assert!(ofl.contains("Copyright (c) 2009-2010, Design Science, Inc."));
    assert!(ofl.contains("Khan Academy"));
    // The fonts are still there; if they are removed, this file can go too.
    assert!(
        root()
            .join("assets/vendor/katex/fonts/KaTeX_Main-Regular.woff2")
            .is_file()
    );
}

#[test]
fn the_mermaid_notices_name_the_bundled_libraries() {
    let notices = read("assets/vendor/mermaid/THIRD-PARTY-NOTICES.txt");
    assert!(notices.starts_with("Third-party notices: libraries inside mermaid\n"));
    // DOMPurify (MPL-2.0 OR Apache-2.0) and ELK (EPL-2.0, with the source address) are
    // inside the Mermaid files; their remaining license comments and file names show it.
    assert!(notices.contains("\ndompurify "), "DOMPurify is missing");
    assert!(notices.contains("Apache License"), "no Apache-2.0 text");
    assert!(
        notices.contains("\nelkjs 0.9.3  EPL-2.0"),
        "elkjs is missing"
    );
    assert!(notices.contains("Eclipse Public License"), "no EPL text");
    assert!(
        notices.contains("https://github.com/kieler/elkjs"),
        "no ELK source address"
    );
    assert!(
        root()
            .join("assets/vendor/mermaid/chunks/mermaid.esm.min")
            .read_dir()
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("elk-")),
        "the ELK chunk is gone; update REVIEWED_PACKAGES in tools/notices.mjs"
    );
}

#[test]
fn license_files_contain_no_local_paths() {
    // The notices are made from files below the user's home folder; no path may leak.
    for relative in [
        "LICENSE",
        "assets/vendor/katex/OFL.txt",
        "assets/vendor/mermaid/THIRD-PARTY-NOTICES.txt",
        "tools/license-texts/webview2-rs.txt",
    ] {
        let text = read(relative);
        for needle in [
            r"C:\Users",
            "/Users/",
            "/home/",
            r"AppData\",
            ".cargo/registry",
        ] {
            assert!(!text.contains(needle), "{relative} contains {needle}");
        }
    }
}
