//! FFmpeg discovery and probing.
//!
//! This module — and Task 21's encoder built on top of it — is one of only two
//! places in the codebase permitted to spawn a process. The binary path comes
//! from the `TERMOTION_FFMPEG` environment variable or `PATH`, never from
//! scenario YAML: letting a scenario influence what gets executed would turn a
//! declarative animation format into an execution vector.

use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;

use termotion_core::{Frame, OutputConfig, OutputFormat, Size};
use thiserror::Error;

use crate::{EncodeError, Encoder};

/// Environment variable used to pin a specific FFmpeg build, checked before `PATH`.
const TERMOTION_FFMPEG_ENV: &str = "TERMOTION_FFMPEG";

/// Bare command name looked up on `PATH` when `TERMOTION_FFMPEG` is unset.
const DEFAULT_FFMPEG_COMMAND: &str = "ffmpeg";

/// Name of the VP9 encoder as it appears in `ffmpeg -encoders` output, used for
/// WebM.
pub const VP9_ENCODER: &str = "libvpx-vp9";

/// Name of the H.264 encoder as it appears in `ffmpeg -encoders` output, used for
/// MP4.
pub const H264_ENCODER: &str = "libx264";

/// Column index of the encoder name in a line of `ffmpeg -encoders` output, once
/// split on whitespace (column 0 is the capability-flags field, e.g. `V....D`).
const ENCODER_NAME_COLUMN: usize = 1;

#[derive(Debug, Error)]
pub enum FfmpegError {
    #[error("ffmpeg not found at `{path}`")]
    NotFound { path: String },
    #[error("ffmpeg failed to start: {0}")]
    Spawn(String),
    #[error("ffmpeg exited with status {status}:\n{stderr}")]
    Failed { status: String, stderr: String },
    #[error("ffmpeg was built without the `{0}` encoder")]
    MissingEncoder(&'static str),
}

/// Result of probing an FFmpeg binary for its version and encoder support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegInfo {
    pub version: String,
    pub has_vp9: bool,
    pub has_h264: bool,
}

/// A located FFmpeg binary. This is the only place in the codebase, besides the
/// encoder itself (Task 21), that spawns a process.
#[derive(Debug, Clone)]
pub struct Ffmpeg {
    binary: PathBuf,
}

impl Ffmpeg {
    /// Locates an FFmpeg binary. `TERMOTION_FFMPEG` wins over `PATH` so a user can
    /// pin a specific build; the candidate is verified by actually invoking it
    /// with `-version` rather than merely checking for a file at that path, since
    /// `PATH` lookup for a bare command name has to happen via the shell/OS
    /// regardless.
    pub fn discover() -> Result<Self, FfmpegError> {
        let candidate = std::env::var(TERMOTION_FFMPEG_ENV)
            .unwrap_or_else(|_| DEFAULT_FFMPEG_COMMAND.to_string());

        let ok = Command::new(&candidate)
            .arg("-version")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);

        if !ok {
            return Err(FfmpegError::NotFound { path: candidate });
        }
        Ok(Ffmpeg {
            binary: PathBuf::from(candidate),
        })
    }

    /// The resolved binary path or command name, as it will be passed to `Command`.
    pub fn binary(&self) -> &PathBuf {
        &self.binary
    }

    /// Probes the binary's version banner and encoder listing.
    pub fn probe(&self) -> Result<FfmpegInfo, FfmpegError> {
        let version_output = Command::new(&self.binary)
            .arg("-version")
            .output()
            .map_err(|err| FfmpegError::Spawn(err.to_string()))?;
        let banner = String::from_utf8_lossy(&version_output.stdout);

        let encoders_output = Command::new(&self.binary)
            .args(["-hide_banner", "-encoders"])
            .output()
            .map_err(|err| FfmpegError::Spawn(err.to_string()))?;
        let listing = String::from_utf8_lossy(&encoders_output.stdout);

        Ok(FfmpegInfo {
            version: parse_version(&banner).unwrap_or_else(|| "unknown".to_string()),
            has_vp9: has_encoder(&listing, VP9_ENCODER),
            has_h264: has_encoder(&listing, H264_ENCODER),
        })
    }
}

/// Extracts the version token from an `ffmpeg -version` banner's first line, e.g.
/// `ffmpeg version 8.0.1 Copyright ...` -> `8.0.1`, or `ffmpeg version
/// n6.1.1-1ubuntu1 Copyright ...` -> `n6.1.1-1ubuntu1`. Distro builds prefix the
/// version with `n` and a suffix, so this only trims the known-fixed banner
/// prefix and takes the next whitespace-delimited token, tolerating either shape.
fn parse_version(banner: &str) -> Option<String> {
    banner
        .lines()
        .next()?
        .strip_prefix("ffmpeg version ")?
        .split_whitespace()
        .next()
        .map(str::to_string)
}

/// Checks whether `name` appears as the *encoder name column* of an `ffmpeg
/// -encoders` listing line, not merely as a substring anywhere in the line — a
/// substring search would false-positive on e.g. `libx264rgb`'s description
/// mentioning another codec, or on `libx264rgb` itself when searching for
/// `libx264`.
fn has_encoder(listing: &str, name: &str) -> bool {
    listing
        .lines()
        .any(|line| line.split_whitespace().nth(ENCODER_NAME_COLUMN) == Some(name))
}

/// The `-crf` flag name, shared by both VP9 and H.264 encodes; the value that
/// follows it is `OutputConfig.quality`, set by the caller rather than a
/// constant here.
const CRF_ARG: &str = "-crf";

/// Streams raw RGBA frames into an FFmpeg subprocess over its stdin:
///
/// ```text
/// webm: -y -f rawvideo -pix_fmt rgba -s WxH -r FPS -i -
///       -c:v libvpx-vp9 -pix_fmt yuv420p -crf Q -b:v 0 <out>
/// mp4:  -y -f rawvideo -pix_fmt rgba -s WxH -r FPS -i -
///       -c:v libx264 -pix_fmt yuv420p -crf Q -movflags +faststart <out>
/// ```
///
/// This — and `Ffmpeg::discover`/`probe` above — are the only places in the
/// codebase permitted to spawn a process (see the module doc comment).
///
/// FFmpeg's stderr is drained by a background thread for the entire lifetime
/// of the child, not just at `finish()`: FFmpeg writes a periodic progress
/// line to stderr throughout the encode, and on a long render that pipe fills
/// its OS buffer and blocks FFmpeg *while we're still writing frames to it*.
/// Without a concurrent reader, that deadlocks `push_frame`'s `write_all`
/// silently, with no error and no timeout — timing out the frame pump itself
/// would abort perfectly valid multi-minute encodes, so draining is the fix,
/// not a deadline.
pub struct FfmpegEncoder {
    child: Option<Child>,
    /// Joined in `finish()` (or by `Drop`, if `finish()` never ran) to recover
    /// everything FFmpeg wrote to stderr, for error messages.
    stderr_reader: Option<JoinHandle<Vec<u8>>>,
    size: Size,
    path: std::path::PathBuf,
    /// Set once `finish()` has run (successfully or not), so `Drop` knows the
    /// child was already reaped and the output file's fate already decided.
    finished: bool,
    /// True when `config.path` did not exist before this encoder spawned
    /// FFmpeg, meaning this encoder — and nothing else — is responsible for
    /// whatever ends up at that path. `Drop`'s cleanup only ever removes a
    /// file when this is true, so a pre-existing file the user pointed at
    /// (the `overwrite: true` case) is never deleted out from under them.
    owns_output_path: bool,
}

impl Default for FfmpegEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FfmpegEncoder {
    pub fn new() -> Self {
        FfmpegEncoder {
            child: None,
            stderr_reader: None,
            size: Size {
                width: 0,
                height: 0,
            },
            path: std::path::PathBuf::new(),
            finished: false,
            owns_output_path: false,
        }
    }

    /// Builds the FFmpeg argument list for `config`, per the command lines
    /// documented on this struct. `-hide_banner` drops FFmpeg's build-config
    /// banner (irrelevant noise ahead of the actual error on a failure) while
    /// leaving its per-frame progress line on stderr intact.
    fn args(config: &OutputConfig) -> Vec<String> {
        let fps = format!("{}/{}", config.fps.num(), config.fps.den());
        let mut args: Vec<String> = vec![
            "-hide_banner".into(),
            "-y".into(),
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            "rgba".into(),
            "-s".into(),
            format!("{}x{}", config.size.width, config.size.height),
            "-r".into(),
            fps,
            "-i".into(),
            "-".into(),
        ];

        match config.format {
            OutputFormat::WebM => {
                args.extend([
                    "-c:v".into(),
                    VP9_ENCODER.into(),
                    "-pix_fmt".into(),
                    "yuv420p".into(),
                    CRF_ARG.into(),
                    config.quality.to_string(),
                    "-b:v".into(),
                    "0".into(),
                ]);
            }
            OutputFormat::Mp4 => {
                args.extend([
                    "-c:v".into(),
                    H264_ENCODER.into(),
                    "-pix_fmt".into(),
                    "yuv420p".into(),
                    CRF_ARG.into(),
                    config.quality.to_string(),
                    "-movflags".into(),
                    "+faststart".into(),
                ]);
            }
            OutputFormat::Png | OutputFormat::Gif => {}
        }

        // Explicitly prefixed with FFmpeg's `file:` protocol so a scenario's
        // `output.path` cannot be reinterpreted as another of FFmpeg's URL
        // protocols (e.g. `tcp://host:port`, `pipe:`) just because it happens
        // to look like one. `output.path` originates from scenario YAML
        // (`termotion_schema::resolve`), which also rejects any path whose
        // filename begins with `-` so it cannot be mistaken for another
        // option in this argument vector — but the `file:` prefix is the
        // guard against protocol reinterpretation specifically, independent
        // of that check.
        args.push(format!("file:{}", config.path.display()));
        args
    }
}

impl FfmpegEncoder {
    /// Removes the output file if — and only if — this encoder is the one
    /// that put it there. Called on any abnormal end (a failed `finish()`, or
    /// a `Drop` before `finish()` ever ran) so a failure is never accompanied
    /// by a truncated-but-playable file sitting at the path the user asked
    /// for. Never touches a file that already existed before `begin()`.
    fn remove_owned_output(&self) {
        if self.owns_output_path {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Collects everything the background reader thread captured from
    /// FFmpeg's stderr. Best-effort: a poisoned/failed thread yields empty
    /// output rather than panicking, since a diagnostic missing detail is far
    /// better than the caller's real error being replaced by a panic.
    fn collect_stderr(&mut self) -> Vec<u8> {
        self.stderr_reader
            .take()
            .and_then(|handle| handle.join().ok())
            .unwrap_or_default()
    }
}

impl Encoder for FfmpegEncoder {
    fn begin(&mut self, config: &OutputConfig) -> Result<(), EncodeError> {
        if config.transparent {
            // Alpha (`yuva420p`, `-auto-alt-ref 0`, the `alpha_mode` metadata tag)
            // is a later milestone. Silently producing an opaque file here would be
            // worse than failing: the user would not discover it until the video
            // was composited over another and the background came out solid black.
            return Err(EncodeError::Ffmpeg(
                "transparent output is not supported yet (WebM/MP4 alpha lands in a later milestone)"
                    .to_string(),
            ));
        }
        if !config.overwrite && config.path.exists() {
            return Err(EncodeError::OutputExists {
                path: config.path.clone(),
            });
        }

        // Recorded before FFmpeg is spawned, so a stale `-y` truncation from a
        // previous run of the process can't be mistaken for "we created this".
        let owns_output_path = !config.path.exists();

        let ffmpeg = Ffmpeg::discover().map_err(|err| EncodeError::Ffmpeg(err.to_string()))?;
        let mut child = Command::new(ffmpeg.binary())
            .args(Self::args(config))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| EncodeError::Io {
                path: config.path.clone(),
                source: err,
            })?;

        // Drain stderr concurrently with the frame pump. FFmpeg writes a
        // progress line to stderr throughout the encode; without a reader
        // running the whole time, a long render fills the pipe's OS buffer
        // and FFmpeg blocks writing to it — which blocks it reading stdin —
        // which deadlocks `push_frame` below, silently, with no timeout.
        let mut stderr_pipe = child.stderr.take().ok_or_else(|| {
            EncodeError::Ffmpeg("ffmpeg did not provide a stderr pipe".to_string())
        })?;
        let stderr_reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr_pipe.read_to_end(&mut buf);
            buf
        });

        self.child = Some(child);
        self.stderr_reader = Some(stderr_reader);
        self.size = config.size;
        self.path = config.path.clone();
        self.finished = false;
        self.owns_output_path = owns_output_path;
        Ok(())
    }

    fn push_frame(&mut self, frame: &Frame) -> Result<(), EncodeError> {
        if frame.width() != self.size.width || frame.height() != self.size.height {
            return Err(EncodeError::FrameSizeMismatch {
                expected: self.size,
                got: Size {
                    width: frame.width(),
                    height: frame.height(),
                },
            });
        }

        let write_result = {
            let child = self
                .child
                .as_mut()
                .ok_or_else(|| EncodeError::Ffmpeg("encoder was not started".to_string()))?;
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| EncodeError::Ffmpeg("ffmpeg stdin is closed".to_string()))?;
            stdin.write_all(frame.data())
        };

        match write_result {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::BrokenPipe => {
                // FFmpeg exited before consuming every frame (bad arguments, an
                // unwritable output path, ...). A bare "broken pipe" tells the user
                // nothing; reap the child and surface its stderr instead, which is
                // what actually explains the failure.
                match self.finish() {
                    Ok(()) => {
                        // `finish()` succeeding here does not mean the render
                        // succeeded — the pipe closed before every frame was
                        // written, so whatever FFmpeg wrote is incomplete.
                        // `finish()`'s own truncated-output guard only fires
                        // on a non-zero exit status, so it's reproduced here
                        // for the "exited zero but consumed too little" case.
                        self.remove_owned_output();
                        Err(EncodeError::Ffmpeg(
                            "ffmpeg's stdin pipe closed unexpectedly, but it exited successfully"
                                .to_string(),
                        ))
                    }
                    Err(finish_err) => Err(finish_err),
                }
            }
            Err(err) => Err(EncodeError::Io {
                path: self.path.clone(),
                source: err,
            }),
        }
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        let Some(mut child) = self.child.take() else {
            self.finished = true;
            return Ok(());
        };

        // Closing stdin is what tells ffmpeg to flush and exit.
        drop(child.stdin.take());

        let status = child.wait().map_err(|err| EncodeError::Io {
            path: self.path.clone(),
            source: err,
        })?;
        // Only set once the child has actually been reaped: if `wait()` above
        // errors, the `?` returns before this line runs, so `finished` stays
        // false and `Drop` still knows this encoder needs its own cleanup
        // (kill/reap/remove) rather than assuming `finish()` already handled it.
        self.finished = true;
        let stderr = self.collect_stderr();

        if !status.success() {
            // Never leave a truncated-but-playable file behind after reporting
            // a failure to the caller.
            self.remove_owned_output();
            // FFmpeg's stderr is the only useful diagnostic here; never swallow it.
            return Err(EncodeError::Ffmpeg(format!(
                "ffmpeg exited with {status}:\n{}",
                String::from_utf8_lossy(&stderr)
            )));
        }
        Ok(())
    }
}

impl Drop for FfmpegEncoder {
    /// Guards against two failure modes when the encoder is dropped without
    /// `finish()` having run — a render error mid-loop, an early return, a
    /// panic:
    ///
    /// 1. `std::process::Child` neither kills nor reaps its process on drop,
    ///    so the FFmpeg child would otherwise linger as a zombie until the
    ///    whole CLI process exits (or indefinitely, for a long-lived library
    ///    consumer of `encoder_for`).
    /// 2. `ChildStdin` closes on drop, which — left alone — tells FFmpeg to
    ///    finalize normally, writing a truncated-but-playable file to the
    ///    user's output path *after* we already reported the render as
    ///    failed. Killing the child instead of merely dropping its stdin
    ///    avoids that.
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(handle) = self.stderr_reader.take() {
            let _ = handle.join();
        }
        self.remove_owned_output();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Serializes tests that mutate the process-global `TERMOTION_FFMPEG`
    /// variable. Cargo runs unit tests in parallel threads within one process, so
    /// without this, another test reading/setting the same variable could race.
    /// As of this writing no other test in this crate touches the variable, but
    /// the guard is kept so a future test doesn't reintroduce the hazard silently.
    static FFMPEG_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn parses_the_version_banner() {
        let banner = "ffmpeg version 8.0.1 Copyright (c) 2000-2025 the FFmpeg developers\nbuilt with Apple clang\n";
        assert_eq!(parse_version(banner).as_deref(), Some("8.0.1"));
    }

    #[test]
    fn parses_distro_version_strings() {
        let banner = "ffmpeg version n6.1.1-1ubuntu1 Copyright (c)\n";
        assert_eq!(parse_version(banner).as_deref(), Some("n6.1.1-1ubuntu1"));
    }

    #[test]
    fn unrecognized_banners_yield_none() {
        assert!(parse_version("not ffmpeg at all").is_none());
    }

    #[test]
    fn detects_encoders_in_the_encoder_listing() {
        let listing =
            " V....D libvpx-vp9           libvpx VP9\n V..... libx264              libx264 H.264\n";
        assert!(has_encoder(listing, "libvpx-vp9"));
        assert!(has_encoder(listing, "libx264"));
        assert!(!has_encoder(listing, "libaom-av1"));
    }

    #[test]
    fn encoder_name_match_is_column_based_not_substring() {
        // `libx264rgb`'s line must not make a `libx264` lookup succeed, and a
        // description mentioning `libx264` in prose must not either.
        let listing = " V....D libx264rgb           libx264 H.264 RGB\n V..... libaom-av1           libaom AV1, replaces libx264 in some builds\n";
        assert!(!has_encoder(listing, "libx264"));
    }

    #[test]
    fn the_output_argument_is_prefixed_with_the_file_protocol() {
        // Scenario data (`output.path`) reaches this argument vector; the
        // `file:` prefix keeps it from being reinterpreted as another
        // FFmpeg URL protocol (network, pipe, ...) no matter what it looks
        // like.
        let config = OutputConfig {
            format: OutputFormat::WebM,
            codec: None,
            path: std::path::PathBuf::from("tcp://127.0.0.1:9999"),
            size: Size {
                width: 4,
                height: 2,
            },
            fps: termotion_core::Fps::from_integer(30),
            transparent: false,
            quality: 32,
            overwrite: true,
        };
        let args = FfmpegEncoder::args(&config);
        assert_eq!(
            args.last().map(String::as_str),
            Some("file:tcp://127.0.0.1:9999")
        );
    }

    #[test]
    fn discovery_prefers_the_environment_variable() {
        let _guard = FFMPEG_ENV_LOCK.lock().unwrap();
        // A deliberately invalid override must surface as NotFound, proving the
        // env var was consulted rather than silently falling back to PATH.
        std::env::set_var(TERMOTION_FFMPEG_ENV, "/nonexistent/ffmpeg-binary");
        let result = Ffmpeg::discover();
        std::env::remove_var(TERMOTION_FFMPEG_ENV);
        assert!(matches!(result, Err(FfmpegError::NotFound { .. })));
    }
}
