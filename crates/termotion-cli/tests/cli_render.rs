use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn outdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("termotion-render-{tag}"));
    let _ = fs::remove_dir_all(&dir);
    dir
}

#[test]
fn renders_a_png_sequence_with_the_expected_frame_count() {
    let out = outdir("png");
    Command::cargo_bin("termotion")
        .unwrap()
        .args([
            "render",
            fixture("basic-write.yaml").to_str().unwrap(),
            "--format",
            "png",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();

    // "hello world" is 11 graphemes at 100ms => 1.1s duration => 11 frames at 10fps.
    let count = fs::read_dir(&out).unwrap().count();
    assert_eq!(count, 11, "unexpected frame count");
    assert!(out.join("000001.png").is_file());
}

#[test]
fn cli_overrides_change_the_output_dimensions() {
    let out = outdir("override");
    // `basic-write.yaml` pins its terminal region to exactly its canvas size
    // (0,0,480,240), so the override must grow the canvas, not shrink it below
    // that — otherwise the terminal would no longer fit and the scenario would
    // fail resolution (E015) rather than exercise the override path.
    Command::cargo_bin("termotion")
        .unwrap()
        .args([
            "render",
            fixture("basic-write.yaml").to_str().unwrap(),
            "--format",
            "png",
            "--output",
            out.to_str().unwrap(),
            "--width",
            "960",
            "--height",
            "480",
            "--fps",
            "5",
        ])
        .assert()
        .success();

    let file = std::io::BufReader::new(fs::File::open(out.join("000001.png")).unwrap());
    let reader = png::Decoder::new(file).read_info().unwrap();
    assert_eq!(reader.info().width, 960);
    assert_eq!(reader.info().height, 480);
}

#[test]
fn rendering_an_invalid_scenario_exits_four_without_writing_frames() {
    let dir = std::env::temp_dir().join("termotion-render-invalid");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let scene = dir.join("scene.yaml");
    fs::write(
        &scene,
        "version: 1\ntimeline:\n  - type: pause\n    duration: 0ms\n",
    )
    .unwrap();

    let out = dir.join("frames");
    Command::cargo_bin("termotion")
        .unwrap()
        .args([
            "render",
            scene.to_str().unwrap(),
            "--format",
            "png",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .code(4);

    assert!(
        !out.exists(),
        "no frames should be written for an invalid scenario"
    );
}

#[test]
fn a_missing_font_reports_error_e020_with_a_hint() {
    let dir = std::env::temp_dir().join("termotion-render-font");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let scene = dir.join("scene.yaml");
    fs::write(
        &scene,
        "version: 1\nfont:\n  family: Definitely Not Installed 12345\ntimeline:\n  - type: write\n    text: x\n",
    )
    .unwrap();

    Command::cargo_bin("termotion")
        .unwrap()
        .args([
            "render",
            scene.to_str().unwrap(),
            "--format",
            "png",
            "--output",
            dir.join("f").to_str().unwrap(),
        ])
        .assert()
        .code(5)
        .stderr(predicates::str::contains("error[E020]"))
        .stderr(predicates::str::contains("font.path"));

    assert!(
        !dir.join("f").exists(),
        "no frames should be written when the font cannot be resolved"
    );
}

#[test]
fn refuses_to_overwrite_without_the_flag() {
    let out = outdir("no-overwrite");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("000001.png"), b"stale").unwrap();

    Command::cargo_bin("termotion")
        .unwrap()
        .args([
            "render",
            fixture("basic-write.yaml").to_str().unwrap(),
            "--format",
            "png",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("error[E030]"));
}

#[test]
fn overwrite_flag_replaces_a_stale_output_directory() {
    let out = outdir("overwrite");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("000001.png"), b"stale").unwrap();
    // A frame index beyond what this render will produce (11 frames, see
    // `renders_a_png_sequence_with_the_expected_frame_count`), simulating
    // leftovers from a longer previous render into the same directory.
    fs::write(out.join("000099.png"), b"stale-long-tail").unwrap();

    Command::cargo_bin("termotion")
        .unwrap()
        .args([
            "render",
            fixture("basic-write.yaml").to_str().unwrap(),
            "--format",
            "png",
            "--output",
            out.to_str().unwrap(),
            "--overwrite",
        ])
        .assert()
        .success();

    let data = fs::read(out.join("000001.png")).unwrap();
    assert_ne!(data, b"stale", "the stale frame should have been replaced");
    assert!(
        !out.join("000099.png").exists(),
        "a stale frame from a longer previous render must not survive --overwrite"
    );
}
