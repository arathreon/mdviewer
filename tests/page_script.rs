//! Runs the JavaScript tests of the page scripts (`assets/viewer.js`, `assets/render.mjs`)
//! with Node.js, so that `cargo test` runs all tests. Node.js must be installed.

use std::process::Command;

#[test]
fn page_script_tests_pass() {
    let output = Command::new("node")
        .args([
            "--test",
            "tests/js/viewer.test.mjs",
            "tests/js/render.test.mjs",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cannot start Node.js; it is necessary for the page script tests");
    assert!(
        output.status.success(),
        "page script tests failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
