use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;

fn scenario(tag: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("termotion-inspect-{tag}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("scene.yaml");
    fs::write(&path, body).unwrap();
    path
}

#[test]
fn inspect_prints_timestamped_events() {
    let path = scenario(
        "typing",
        "version: 1\ntimeline:\n  - type: write\n    text: zom\n    speed: 45ms\n",
    );

    Command::cargo_bin("termotion")
        .unwrap()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicates::str::contains("0ms      type \"z\""))
        .stdout(predicates::str::contains("45ms     type \"o\""))
        .stdout(predicates::str::contains("90ms     type \"m\""));
}

#[test]
fn inspect_reports_the_summary() {
    let path = scenario(
        "summary",
        "version: 1\ncanvas:\n  fps: 30\ntimeline:\n  - type: pause\n    duration: 1s\n",
    );

    Command::cargo_bin("termotion")
        .unwrap()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicates::str::contains("Duration: 1.00s"))
        .stdout(predicates::str::contains("Frames: 30"));
}

#[test]
fn inspect_fails_on_an_invalid_scenario() {
    let path = scenario(
        "bad",
        "version: 1\ntimeline:\n  - type: pause\n    duration: 0ms\n",
    );
    Command::cargo_bin("termotion")
        .unwrap()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .code(4);
}

#[test]
fn inspect_rejects_a_missing_font_instead_of_silently_estimating_the_grid() {
    // Regression test: `inspect` used to derive its grid from
    // `GridSpec::estimate` without ever loading the configured font, so a
    // scenario naming a font that does not exist would print a grid anyway
    // (one computed from a ratio that doesn't apply to whatever font would
    // actually have been used). Now that it loads the real font first (the
    // same as `validate` and `render`), a missing font must surface here too.
    let path = scenario(
        "missing-font",
        "version: 1\nfont:\n  family: Definitely Not Installed 12345\ntimeline: []\n",
    );
    Command::cargo_bin("termotion")
        .unwrap()
        .args(["inspect", path.to_str().unwrap()])
        .assert()
        .code(5)
        .stderr(predicates::str::contains("error[E020]"));
}
