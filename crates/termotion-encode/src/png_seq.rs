use std::io::BufWriter;
use std::path::PathBuf;

use termotion_core::{Frame, OutputConfig, Size};

use crate::{EncodeError, Encoder};

/// Width of the zero-padded frame index in output filenames, e.g. `000001.png`.
/// Six digits supports sequences up to 999,999 frames (~9.25 hours at 30fps)
/// without the filename length changing partway through an encode.
const FRAME_INDEX_WIDTH: usize = 6;

/// Writes a numbered PNG per frame. `OutputConfig.path` is a directory.
pub struct PngSequenceEncoder {
    dir: PathBuf,
    size: Size,
    index: u64,
}

impl Default for PngSequenceEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl PngSequenceEncoder {
    pub fn new() -> Self {
        PngSequenceEncoder {
            dir: PathBuf::new(),
            size: Size {
                width: 0,
                height: 0,
            },
            index: 0,
        }
    }

    pub fn frames_written(&self) -> u64 {
        self.index
    }
}

/// Removes every file in `dir` whose name matches this encoder's own
/// `{:06}.png`-style naming scheme (see `FRAME_INDEX_WIDTH`), leaving
/// anything else in the directory untouched. A missing directory is not an
/// error: `begin` creates it immediately after this call.
fn clear_frame_files(dir: &std::path::Path) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };

    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        if is_frame_file_name(&name) {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// True for names of the exact shape this encoder writes: `FRAME_INDEX_WIDTH`
/// ASCII digits, then `.png`.
fn is_frame_file_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some(digits) = name.strip_suffix(".png") else {
        return false;
    };
    digits.len() == FRAME_INDEX_WIDTH && digits.bytes().all(|b| b.is_ascii_digit())
}

impl Encoder for PngSequenceEncoder {
    fn begin(&mut self, config: &OutputConfig) -> Result<(), EncodeError> {
        if !config.overwrite && config.path.exists() {
            let occupied = std::fs::read_dir(&config.path)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(false);
            if occupied {
                return Err(EncodeError::OutputExists {
                    path: config.path.clone(),
                });
            }
        }

        std::fs::create_dir_all(&config.path).map_err(|err| EncodeError::Io {
            path: config.path.clone(),
            source: err,
        })?;

        // `--overwrite` must mean "this directory holds exactly this render's
        // frames" — not "add to, or partially replace, whatever is already
        // there". Re-rendering a shorter scenario into the same directory
        // without this leaves the previous render's trailing frames in
        // place, so `ffmpeg -i %06d.png` (or any consumer of the sequence)
        // would encode a mixed, corrupt-looking result. Only frame files
        // matching this encoder's own naming scheme are removed, so an
        // `--overwrite` into a directory that happens to hold unrelated
        // files doesn't touch them.
        if config.overwrite {
            clear_frame_files(&config.path).map_err(|err| EncodeError::Io {
                path: config.path.clone(),
                source: err,
            })?;
        }

        self.dir = config.path.clone();
        self.size = config.size;
        self.index = 0;
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

        self.index += 1;
        let path = self.dir.join(format!(
            "{:0width$}.png",
            self.index,
            width = FRAME_INDEX_WIDTH
        ));
        let file = std::fs::File::create(&path).map_err(|err| EncodeError::Io {
            path: path.clone(),
            source: err,
        })?;

        let mut encoder = png::Encoder::new(BufWriter::new(file), frame.width(), frame.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);

        let mut writer = encoder
            .write_header()
            .map_err(|err| EncodeError::Png(err.to_string()))?;
        writer
            .write_image_data(frame.data())
            .map_err(|err| EncodeError::Png(err.to_string()))?;
        writer
            .finish()
            .map_err(|err| EncodeError::Png(err.to_string()))?;

        Ok(())
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termotion_core::{Color, Fps, OutputFormat, Size};

    fn config(dir: &std::path::Path) -> OutputConfig {
        OutputConfig {
            format: OutputFormat::Png,
            codec: None,
            path: dir.to_path_buf(),
            size: Size {
                width: 4,
                height: 2,
            },
            fps: Fps::from_integer(30),
            transparent: false,
            quality: 32,
            overwrite: true,
        }
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("termotion-png-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn frame(color: Color) -> Frame {
        let mut frame = Frame::new(4, 2);
        frame.fill(color);
        frame
    }

    #[test]
    fn writes_one_file_per_frame_with_six_digit_names() {
        let dir = tempdir("names");
        let mut encoder = PngSequenceEncoder::new();
        encoder.begin(&config(&dir)).unwrap();
        for _ in 0..3 {
            encoder.push_frame(&frame(Color::rgb(1, 2, 3))).unwrap();
        }
        encoder.finish().unwrap();

        assert!(dir.join("000001.png").is_file());
        assert!(dir.join("000002.png").is_file());
        assert!(dir.join("000003.png").is_file());
        assert!(!dir.join("000004.png").exists());
    }

    #[test]
    fn written_pngs_round_trip_with_the_right_pixels() {
        let dir = tempdir("roundtrip");
        let mut encoder = PngSequenceEncoder::new();
        encoder.begin(&config(&dir)).unwrap();
        encoder
            .push_frame(&frame(Color::rgba(10, 20, 30, 255)))
            .unwrap();
        encoder.finish().unwrap();

        let file = std::io::BufReader::new(std::fs::File::open(dir.join("000001.png")).unwrap());
        let decoder = png::Decoder::new(file);
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();

        assert_eq!(info.width, 4);
        assert_eq!(info.height, 2);
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(&buf[0..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn alpha_is_preserved_for_transparent_frames() {
        let dir = tempdir("alpha");
        let mut cfg = config(&dir);
        cfg.transparent = true;
        let mut encoder = PngSequenceEncoder::new();
        encoder.begin(&cfg).unwrap();
        encoder.push_frame(&Frame::new(4, 2)).unwrap();
        encoder.finish().unwrap();

        let file = std::io::BufReader::new(std::fs::File::open(dir.join("000001.png")).unwrap());
        let mut reader = png::Decoder::new(file).read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        reader.next_frame(&mut buf).unwrap();
        assert_eq!(buf[3], 0, "alpha must survive the round trip");
    }

    #[test]
    fn creates_the_output_directory_when_missing() {
        let dir = tempdir("mkdir").join("nested").join("frames");
        let mut encoder = PngSequenceEncoder::new();
        encoder.begin(&config(&dir)).unwrap();
        encoder.push_frame(&frame(Color::BLACK)).unwrap();
        encoder.finish().unwrap();
        assert!(dir.join("000001.png").is_file());
    }

    #[test]
    fn refuses_to_overwrite_an_existing_directory_without_the_flag() {
        let dir = tempdir("overwrite");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("000001.png"), b"stale").unwrap();

        let mut cfg = config(&dir);
        cfg.overwrite = false;
        let mut encoder = PngSequenceEncoder::new();
        let err = encoder.begin(&cfg).unwrap_err();
        assert!(matches!(err, EncodeError::OutputExists { .. }));
    }

    #[test]
    fn overwrite_clears_stale_frames_from_a_longer_previous_render() {
        let dir = tempdir("stale");

        // First render: 5 frames.
        let mut encoder = PngSequenceEncoder::new();
        encoder.begin(&config(&dir)).unwrap();
        for _ in 0..5 {
            encoder.push_frame(&frame(Color::rgb(1, 2, 3))).unwrap();
        }
        encoder.finish().unwrap();
        assert!(dir.join("000005.png").is_file());

        // Second, shorter render into the same directory with --overwrite.
        let mut encoder = PngSequenceEncoder::new();
        encoder.begin(&config(&dir)).unwrap();
        for _ in 0..2 {
            encoder.push_frame(&frame(Color::rgb(4, 5, 6))).unwrap();
        }
        encoder.finish().unwrap();

        assert!(dir.join("000001.png").is_file());
        assert!(dir.join("000002.png").is_file());
        assert!(
            !dir.join("000005.png").exists(),
            "stale frame from the longer previous render must be cleared"
        );
    }

    #[test]
    fn overwrite_does_not_touch_unrelated_files_in_the_directory() {
        let dir = tempdir("unrelated");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), b"keep me").unwrap();

        let mut encoder = PngSequenceEncoder::new();
        encoder.begin(&config(&dir)).unwrap();
        encoder.push_frame(&frame(Color::rgb(1, 2, 3))).unwrap();
        encoder.finish().unwrap();

        assert!(dir.join("notes.txt").is_file());
    }

    #[test]
    fn a_frame_of_the_wrong_size_is_rejected() {
        let dir = tempdir("size");
        let mut encoder = PngSequenceEncoder::new();
        encoder.begin(&config(&dir)).unwrap();
        let err = encoder.push_frame(&Frame::new(8, 8)).unwrap_err();
        assert!(matches!(err, EncodeError::FrameSizeMismatch { .. }));
    }
}
