//! Acceptance gate.

use std::path::PathBuf;

use assert_cmd::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .unwrap()
        .to_path_buf()
}

#[test]
fn brb_example_renders_a_webm() {
    if termotion_encode::ffmpeg::Ffmpeg::discover().is_err() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }

    let scene = workspace_root().join("examples/brb.yaml");
    let out = std::env::temp_dir().join("termotion-acceptance-brb.webm");
    let _ = std::fs::remove_file(&out);

    Command::cargo_bin("termotion")
        .unwrap()
        .args([
            "render",
            scene.to_str().unwrap(),
            "--output",
            out.to_str().unwrap(),
            "--overwrite",
        ])
        .assert()
        .success();

    let probe = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,codec_name,r_frame_rate",
            "-of",
            "csv=p=0",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let info = String::from_utf8_lossy(&probe.stdout);

    assert!(info.contains("1920,1080"), "expected 1920x1080, got {info}");
    assert!(info.contains("vp9"), "expected vp9, got {info}");
    assert!(info.contains("30/1"), "expected 30fps, got {info}");
}

#[test]
fn the_brb_example_validates() {
    let scene = workspace_root().join("examples/brb.yaml");
    Command::cargo_bin("termotion")
        .unwrap()
        .args(["validate", scene.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn the_starting_soon_example_validates() {
    let scene = workspace_root().join("examples/starting-soon.yaml");
    Command::cargo_bin("termotion")
        .unwrap()
        .args(["validate", scene.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn the_ending_example_validates() {
    let scene = workspace_root().join("examples/ending.yaml");
    Command::cargo_bin("termotion")
        .unwrap()
        .args(["validate", scene.to_str().unwrap()])
        .assert()
        .success();
}
