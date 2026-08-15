use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;

fn scenario(tag: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("termotion-cli-{tag}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("scene.yaml");
    fs::write(&path, body).unwrap();
    path
}

#[test]
fn valid_scenario_exits_zero_and_reports_checks() {
    let path = scenario(
        "ok",
        "version: 1\ntimeline:\n  - type: write\n    text: hello\n",
    );
    Command::cargo_bin("termotion")
        .unwrap()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicates::str::contains("Scenario valid."));
}

#[test]
fn validation_failure_exits_with_code_four() {
    let path = scenario(
        "bad",
        "version: 1\ntimeline:\n  - type: pause\n    duration: 0ms\n",
    );
    Command::cargo_bin("termotion")
        .unwrap()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .code(4)
        .stderr(predicates::str::contains("error[E014]"))
        .stderr(predicates::str::contains("scene.yaml:4:15"));
}

#[test]
fn parse_failure_exits_with_code_three() {
    let path = scenario("syntax", "version: 1\ncanvas:\n  - [unclosed\n");
    Command::cargo_bin("termotion")
        .unwrap()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .code(3);
}

#[test]
fn missing_file_exits_with_code_five() {
    Command::cargo_bin("termotion")
        .unwrap()
        .args(["validate", "/nonexistent/scene.yaml"])
        .assert()
        .code(5);
}

#[test]
fn json_output_is_machine_readable() {
    let path = scenario(
        "json",
        "version: 1\ntimeline:\n  - type: write\n    text: hi\n",
    );
    let output = Command::cargo_bin("termotion")
        .unwrap()
        .args(["validate", path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["valid"], serde_json::Value::Bool(true));
    assert_eq!(parsed["actions"], serde_json::json!(1));
}

#[test]
fn json_output_lists_errors_when_invalid() {
    let path = scenario(
        "json-bad",
        "version: 1\ntimeline:\n  - type: pause\n    duration: 0ms\n",
    );
    let output = Command::cargo_bin("termotion")
        .unwrap()
        .args(["validate", path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["valid"], serde_json::Value::Bool(false));
    assert_eq!(parsed["errors"][0]["code"], "E014");
}

#[test]
fn validate_reports_duration_and_frame_count() {
    let path = scenario(
        "duration",
        "version: 1\ncanvas:\n  fps: 30\ntimeline:\n  - type: pause\n    duration: 2s\n",
    );

    Command::cargo_bin("termotion")
        .unwrap()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicates::str::contains("Duration: 2.00s"))
        .stdout(predicates::str::contains("Frames: 60"));
}

#[test]
fn validate_json_includes_duration_and_frames() {
    let path = scenario(
        "duration-json",
        "version: 1\ncanvas:\n  fps: 30\ntimeline:\n  - type: pause\n    duration: 2s\n",
    );

    let output = Command::cargo_bin("termotion")
        .unwrap()
        .args(["validate", path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["duration_ms"], serde_json::json!(2000));
    assert_eq!(parsed["frames"], serde_json::json!(60));
}

#[test]
fn validate_rejects_a_missing_font() {
    let path = scenario(
        "font",
        "version: 1\nfont:\n  family: Definitely Not Installed 12345\ntimeline: []\n",
    );
    Command::cargo_bin("termotion")
        .unwrap()
        .args(["validate", path.to_str().unwrap()])
        .assert()
        .code(5)
        .stderr(predicates::str::contains("error[E020]"));
}

#[test]
fn validate_json_emits_fps_as_a_lossless_float_not_truncated_integer_division() {
    // `Fps` is an exact `num/den` rational (so e.g. NTSC's 30000/1001 is
    // representable without loss); `canvas.fps` in YAML only accepts a plain
    // integer today, so `den` is always 1 through this path, and 30/1 is a
    // deliberately boring case where naive `num() / den()` integer division
    // would happen to give the same *value* as the fix. What integer
    // division would NOT give is the same *type*: this asserts the JSON
    // field is a float (`is_f64()`), proving the `f64` division path from
    // `f64::from(num) / f64::from(den)` is in use rather than
    // `num() / den()`'s `u32` division, which `serde_json` would have
    // serialized as a bare integer.
    let path = scenario("fps-type", "version: 1\ncanvas:\n  fps: 30\ntimeline: []\n");
    let output = Command::cargo_bin("termotion")
        .unwrap()
        .args(["validate", path.to_str().unwrap(), "--json"])
        .output()
        .unwrap();

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        parsed["fps"].is_f64(),
        "expected a float fps field, got {:?}",
        parsed["fps"]
    );
    assert_eq!(parsed["fps"].as_f64(), Some(30.0));
}

#[test]
fn themes_list_shows_builtins() {
    Command::cargo_bin("termotion")
        .unwrap()
        .args(["themes", "list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("terminal-green"))
        .stdout(predicates::str::contains("zombocoder"));
}

#[test]
fn version_prints_the_crate_version() {
    Command::cargo_bin("termotion")
        .unwrap()
        .arg("version")
        .assert()
        .success()
        .stdout(predicates::str::contains(env!("CARGO_PKG_VERSION")));
}
