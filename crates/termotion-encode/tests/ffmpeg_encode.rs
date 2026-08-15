//! Gated on FFmpeg being installed.

use termotion_core::{Color, Fps, Frame, OutputConfig, OutputFormat, Size};
use termotion_encode::ffmpeg::{Ffmpeg, FfmpegEncoder};
use termotion_encode::Encoder;

fn ffmpeg_available() -> bool {
    Ffmpeg::discover().is_ok()
}

fn out_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("termotion-ffmpeg");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn config(path: std::path::PathBuf, format: OutputFormat) -> OutputConfig {
    OutputConfig {
        format,
        codec: None,
        path,
        size: Size {
            width: 64,
            height: 32,
        },
        fps: Fps::from_integer(10),
        transparent: false,
        quality: 40,
        overwrite: true,
    }
}

fn push_frames(encoder: &mut FfmpegEncoder, count: usize) {
    for i in 0..count {
        let mut frame = Frame::new(64, 32);
        frame.fill(Color::rgb((i * 20) as u8, 40, 60));
        encoder.push_frame(&frame).unwrap();
    }
}

#[test]
fn writes_a_playable_webm() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let path = out_path("test.webm");
    let _ = std::fs::remove_file(&path);

    let mut encoder = FfmpegEncoder::new();
    encoder
        .begin(&config(path.clone(), OutputFormat::WebM))
        .unwrap();
    push_frames(&mut encoder, 10);
    encoder.finish().unwrap();

    let size = std::fs::metadata(&path).unwrap().len();
    assert!(size > 0, "webm must not be empty");

    // ffprobe confirms the container is readable and has the expected geometry.
    let probe = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,codec_name",
            "-of",
            "csv=p=0",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    let info = String::from_utf8_lossy(&probe.stdout);
    assert!(info.contains("vp9"), "expected vp9, got {info}");
    assert!(info.contains("64,32"), "expected 64x32, got {info}");
}

#[test]
fn writes_a_playable_mp4() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg not installed");
        return;
    }
    let path = out_path("test.mp4");
    let _ = std::fs::remove_file(&path);

    let mut encoder = FfmpegEncoder::new();
    encoder
        .begin(&config(path.clone(), OutputFormat::Mp4))
        .unwrap();
    push_frames(&mut encoder, 10);
    encoder.finish().unwrap();

    assert!(std::fs::metadata(&path).unwrap().len() > 0);
}

#[test]
fn a_failing_ffmpeg_surfaces_its_stderr() {
    if !ffmpeg_available() {
        return;
    }
    // An unwritable destination doesn't fail ffmpeg instantly: its rawvideo
    // demuxer reads and buffers one full frame before it even attempts to
    // open the output muxer, so a *single* frame — regardless of size —
    // always writes successfully and the error surfaces through `finish()`'s
    // ordinary non-zero-exit path, proving nothing about the BrokenPipe
    // branch (this was verified directly: a single 1920x1080 frame here
    // still passes even with the BrokenPipe handling in `push_frame` deleted
    // entirely). Once ffmpeg has consumed that first frame, it fails to open
    // the muxer and exits — so the *second* write is the one that lands on a
    // closed pipe. Pushing several frames, well above frame 1, is what
    // actually exercises `push_frame`'s `ErrorKind::BrokenPipe` branch; this
    // was confirmed by reverting that branch to a plain IO-error mapping and
    // observing this exact test fail with "Broken pipe (os error 32)"
    // instead of ffmpeg's real stderr.
    let width = 1920;
    let height = 1080;
    let mut cfg = config("/nonexistent-dir/out.webm".into(), OutputFormat::WebM);
    cfg.size = Size { width, height };

    let mut encoder = FfmpegEncoder::new();
    let result = encoder.begin(&cfg).and_then(|_| {
        let mut frame = Frame::new(width, height);
        frame.fill(Color::BLACK);
        for _ in 0..5 {
            encoder.push_frame(&frame)?;
        }
        encoder.finish()
    });

    let err = result.expect_err("expected an encoder error");
    let message = err.to_string();
    assert!(
        message.contains("No such file or directory") || message.contains("Error opening output"),
        "expected ffmpeg's actual stderr in the error message, got: {message}"
    );
}

#[test]
fn dropping_without_finish_removes_the_partial_output_it_created() {
    if !ffmpeg_available() {
        return;
    }
    let path = out_path("dropped.webm");
    let _ = std::fs::remove_file(&path);

    {
        let mut encoder = FfmpegEncoder::new();
        encoder
            .begin(&config(path.clone(), OutputFormat::WebM))
            .unwrap();
        // Dropped here without ever calling finish() or pushing a frame —
        // simulates a render erroring out mid-loop (a RenderError, a
        // FrameSizeMismatch, a panic). If Drop merely closed stdin, ffmpeg
        // would finalize normally and leave a truncated-but-playable file at
        // this path despite the caller having already seen an error.
    }

    assert!(
        !path.exists(),
        "an encoder dropped without finish() must not leave a file behind"
    );
}

#[test]
fn dropping_without_finish_never_deletes_a_pre_existing_file() {
    if !ffmpeg_available() {
        return;
    }
    let path = out_path("pre-existing.webm");
    std::fs::write(&path, b"not a real webm, just a sentinel").unwrap();

    {
        let mut encoder = FfmpegEncoder::new();
        let cfg = config(path.clone(), OutputFormat::WebM);
        // `begin()` records that the path already existed before spawning
        // ffmpeg; dropping immediately after must not remove it, even though
        // ffmpeg was launched with `-y` and may itself attempt to overwrite
        // it once it opens the output.
        let _ = encoder.begin(&cfg);
    }

    assert!(
        path.exists(),
        "a file the user pointed at before begin() must never be deleted by Drop cleanup"
    );
}

#[test]
fn transparency_is_rejected_until_alpha_support_lands() {
    let mut encoder = FfmpegEncoder::new();
    let mut cfg = config(out_path("alpha.webm"), OutputFormat::WebM);
    cfg.transparent = true;
    assert!(encoder.begin(&cfg).is_err());
}
