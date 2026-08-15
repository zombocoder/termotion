pub mod ffmpeg;
pub mod png_seq;

use std::path::PathBuf;

use termotion_core::{Frame, OutputConfig, OutputFormat, Size};
use thiserror::Error;

pub use ffmpeg::FfmpegEncoder;
pub use png_seq::PngSequenceEncoder;

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("cannot write `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("`{path}` already exists; pass --overwrite to replace it")]
    OutputExists { path: PathBuf },
    #[error("frame is {got:?} but the encoder was configured for {expected:?}")]
    FrameSizeMismatch { expected: Size, got: Size },
    #[error("png encoding failed: {0}")]
    Png(String),
    /// Reserved for Tasks 20-21's FFmpeg-backed WebM/MP4 encoders, so an FFmpeg
    /// process failure is never forced through `Png(String)` and mislabeled.
    #[error("ffmpeg: {0}")]
    Ffmpeg(String),
    #[error("output format {0} is not supported yet")]
    UnsupportedFormat(&'static str),
}

/// Streaming encoder interface. Frames are pushed one at a time and
/// released immediately, so peak memory never scales with the frame count.
pub trait Encoder {
    fn begin(&mut self, config: &OutputConfig) -> Result<(), EncodeError>;
    fn push_frame(&mut self, frame: &Frame) -> Result<(), EncodeError>;
    fn finish(&mut self) -> Result<(), EncodeError>;
}

pub fn encoder_for(config: &OutputConfig) -> Result<Box<dyn Encoder>, EncodeError> {
    match config.format {
        OutputFormat::Png => Ok(Box::new(PngSequenceEncoder::new())),
        OutputFormat::WebM | OutputFormat::Mp4 => Ok(Box::new(FfmpegEncoder::new())),
        OutputFormat::Gif => Err(EncodeError::UnsupportedFormat("gif")),
    }
}
