//! Tests of `.github/scripts/check-release-version.ps1`, the first step of the release
//! workflow. It decides which tags give a release, so each rule has a test here.
//!
//! The tests run the script with PowerShell 7 (`pwsh`), as GitHub does, and give the
//! version with `-Version`, so that the script does not run cargo.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The script under test.
fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/scripts/check-release-version.ps1")
}

/// Runs the script for `tag` and the version 0.1.0. `GITHUB_OUTPUT` is removed: in CI it
/// names the output file of the test step, and the script must not write into it.
fn run(tag: &str, prerelease: bool) -> Output {
    let mut command = Command::new("pwsh");
    command
        .args(["-NoProfile", "-NonInteractive", "-File"])
        .arg(script())
        .args(["-Tag", tag, "-Version", "0.1.0"])
        .env_remove("GITHUB_OUTPUT");
    if prerelease {
        command.arg("-Prerelease");
    }
    command
        .output()
        .expect("PowerShell 7 (pwsh) is necessary for this test")
}

/// The standard output as text, without the line end.
fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned()
}

/// Checks that the script accepts `tag` and gives `release` as the release version.
fn assert_accepted(tag: &str, prerelease: bool, release: &str) {
    let output = run(tag, prerelease);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{tag} (pre-release: {prerelease}): {output:?}"
    );
    assert_eq!(stdout(&output), release, "{tag}");
}

/// Checks that the script rejects `tag` with a GitHub error that contains `reason`.
fn assert_rejected(tag: &str, prerelease: bool, reason: &str) {
    let output = run(tag, prerelease);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{tag} (pre-release: {prerelease}): {output:?}"
    );
    let text = stdout(&output);
    assert!(text.starts_with("::error::"), "{tag}: {text}");
    assert!(text.contains(reason), "{tag}: {text}");
}

#[test]
fn a_release_needs_exactly_the_version() {
    assert_accepted("v0.1.0", false, "0.1.0");
    assert_accepted("0.1.0", false, "0.1.0");
    for tag in ["v0.2.0", "v0.1", "V0.1.0", "vv0.1.0", "v0.1.0.1", "release"] {
        assert_rejected(tag, false, "does not match the version '0.1.0'");
    }
}

#[test]
fn a_release_cannot_have_a_suffix() {
    // The tag of a pre-release, but the box "Set as a pre-release" is not selected.
    assert_rejected("v0.1.0-rc.1", false, "Select 'Set as a pre-release'");
}

#[test]
fn a_prerelease_can_add_a_suffix_to_the_version() {
    assert_accepted("v0.1.0", true, "0.1.0");
    assert_accepted("v0.1.0-rc.1", true, "0.1.0-rc.1");
    assert_accepted("v0.1.0-rc.2", true, "0.1.0-rc.2");
    assert_accepted("0.1.0-beta", true, "0.1.0-beta");
    assert_accepted("v0.1.0-test-zip.3", true, "0.1.0-test-zip.3");
}

#[test]
fn a_prerelease_suffix_must_be_safe() {
    // An empty part, other characters (also for file names), or another version.
    for tag in [
        "v0.1.0-",
        "v0.1.0-rc..1",
        "v0.1.0-rc.",
        "v0.1.0-rc 1",
        "v0.1.0-rc/1",
        "v0.1.0-rc_1",
        "v0.1.0+build",
        "v0.2.0-rc.1",
        "v0.1.0rc1",
    ] {
        assert_rejected(tag, true, "does not match the version '0.1.0'");
    }
}

#[test]
fn the_release_version_goes_to_the_github_output_file() {
    let file = std::env::temp_dir().join(format!(
        "mdviewer test 'é& github-output-{}.txt",
        std::process::id()
    ));
    std::fs::write(&file, "earlier=1\n").unwrap();
    let output = Command::new("pwsh")
        .args(["-NoProfile", "-NonInteractive", "-File"])
        .arg(script())
        .args(["-Tag", "v0.1.0-rc.1", "-Version", "0.1.0", "-Prerelease"])
        .env("GITHUB_OUTPUT", &file)
        .output()
        .expect("PowerShell 7 (pwsh) is necessary for this test");
    let written = std::fs::read(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    // Appended, UTF-8 without a byte order mark; the line end of Windows is accepted.
    let text = String::from_utf8(written).unwrap().replace("\r\n", "\n");
    assert_eq!(text, "earlier=1\nversion=0.1.0-rc.1\n");
}
