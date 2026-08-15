use assert_cmd::Command;

/// FFmpeg presence is environment-dependent; assert on the report's shape, not on a
/// particular machine having it installed.
#[test]
fn doctor_reports_ffmpeg_status() {
    let output = Command::cargo_bin("termotion")
        .unwrap()
        .arg("doctor")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("ffmpeg"),
        "doctor must mention ffmpeg: {text}"
    );
    assert!(text.contains("libvpx-vp9") || text.contains("not found"));
}

#[test]
fn doctor_exits_seven_when_ffmpeg_is_missing() {
    let output = Command::cargo_bin("termotion")
        .unwrap()
        .env("TERMOTION_FFMPEG", "/nonexistent/ffmpeg-binary")
        .arg("doctor")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
}

/// A missing FFmpeg must not make the whole tool look broken: `doctor` must say
/// PNG output still works.
#[test]
fn doctor_mentions_png_works_without_ffmpeg() {
    let output = Command::cargo_bin("termotion")
        .unwrap()
        .env("TERMOTION_FFMPEG", "/nonexistent/ffmpeg-binary")
        .arg("doctor")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.to_lowercase().contains("png"),
        "doctor must mention that PNG output works without ffmpeg: {text}"
    );
}
