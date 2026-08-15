use std::path::PathBuf;

use crate::color::Color;
use crate::geom::{Padding, Rect, Size};
use crate::grid::Overflow;
use crate::style::TextStyle;
use crate::terminal::{CursorStyle, DEFAULT_CURSOR_BLINK};
use crate::time::{Fps, Time};

/// Default canvas width in pixels (the reference 1080p canvas).
pub const DEFAULT_CANVAS_WIDTH: u32 = 1920;
/// Default canvas height in pixels (the reference 1080p canvas).
pub const DEFAULT_CANVAS_HEIGHT: u32 = 1080;
/// Default frame rate. 30fps is smooth enough for terminal typing animation
/// without doubling render and encode cost relative to lower rates.
pub const DEFAULT_FPS: u32 = 30;

/// Maximum allowed canvas width or height, in pixels.
///
/// Set to twice the longer edge of 8K (7680x4320) — the largest resolution in
/// common broadcast/streaming use — so real scenarios, including any
/// reasonable "16K" master, are never affected. At the same time it bounds the
/// single reused frame buffer (`width * height * 4` bytes, see `Frame::new`)
/// to at most 16384 * 16384 * 4 bytes, about 1 GiB: large enough to be a
/// non-issue for legitimate renders, small enough that a scenario cannot make
/// `render` attempt a multi-exabyte allocation (an OOM kill with no
/// diagnostic) while `validate` reported the same scenario as valid. Checked
/// at validation time in `termotion_schema::resolve`, not here — `termotion-core`
/// has no diagnostic type of its own.
pub const MAX_CANVAS_DIMENSION: u32 = 16_384;

#[derive(Debug, Clone, PartialEq)]
pub struct CanvasConfig {
    pub size: Size,
    pub fps: Fps,
    pub background: BackgroundConfig,
    pub transparent: bool,
}

impl Default for CanvasConfig {
    fn default() -> Self {
        CanvasConfig {
            size: Size {
                width: DEFAULT_CANVAS_WIDTH,
                height: DEFAULT_CANVAS_HEIGHT,
            },
            fps: Fps::from_integer(DEFAULT_FPS),
            background: BackgroundConfig::Solid(Color::rgb(8, 11, 9)),
            transparent: false,
        }
    }
}

/// Only `Solid` exists in M1–M4; image and gradient backgrounds are a future addition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackgroundConfig {
    Solid(Color),
}

/// Default gap, in pixels, between the canvas edge and the terminal region on
/// each side. Applied on all four edges, so the default terminal region is
/// the default canvas inset by this amount horizontally and vertically.
pub const DEFAULT_TERMINAL_MARGIN: u32 = 110;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalConfig {
    pub bounds: Rect,
    pub padding: Padding,
    pub overflow: Overflow,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        TerminalConfig {
            bounds: Rect {
                x: DEFAULT_TERMINAL_MARGIN as i32,
                y: DEFAULT_TERMINAL_MARGIN as i32,
                width: DEFAULT_CANVAS_WIDTH - 2 * DEFAULT_TERMINAL_MARGIN,
                height: DEFAULT_CANVAS_HEIGHT - 2 * DEFAULT_TERMINAL_MARGIN,
            },
            padding: Padding::uniform(0),
            overflow: Overflow::Scroll,
        }
    }
}

/// Default font size in pixels, sized to read clearly at 1080p.
pub const DEFAULT_FONT_SIZE: f32 = 40.0;
/// Default line height as a multiple of font size. 1.4 gives typing room to
/// breathe without wasting vertical space in the terminal region.
pub const DEFAULT_LINE_HEIGHT: f32 = 1.4;
/// Default font weight (CSS-style 100-900 scale). 400 is "regular".
pub const DEFAULT_FONT_WEIGHT: u16 = 400;

#[derive(Debug, Clone, PartialEq)]
pub struct FontConfig {
    pub family: String,
    /// An explicit font file, which takes precedence over `family`.
    pub path: Option<PathBuf>,
    pub size: f32,
    pub weight: u16,
    pub line_height: f32,
    pub letter_spacing: f32,
}

impl Default for FontConfig {
    fn default() -> Self {
        FontConfig {
            family: "JetBrains Mono".to_string(),
            path: None,
            size: DEFAULT_FONT_SIZE,
            weight: DEFAULT_FONT_WEIGHT,
            line_height: DEFAULT_LINE_HEIGHT,
            letter_spacing: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptConfig {
    pub user: String,
    pub host: String,
    pub path: String,
    pub symbol: String,
}

impl Default for PromptConfig {
    fn default() -> Self {
        PromptConfig {
            user: "user".to_string(),
            host: "localhost".to_string(),
            path: "~".to_string(),
            symbol: "$".to_string(),
        }
    }
}

impl PromptConfig {
    /// Renders `user@host:path$ ` as palette-colored spans.
    ///
    /// The separators inherit the surrounding segment's color so the prompt reads
    /// as one unit rather than as five disconnected pieces.
    pub fn spans(&self, palette: &Palette) -> Vec<(String, TextStyle)> {
        let style = |fg: Color| TextStyle {
            fg,
            ..TextStyle::default()
        };
        vec![
            (self.user.clone(), style(palette.prompt_user)),
            ("@".to_string(), style(palette.prompt_user)),
            (self.host.clone(), style(palette.prompt_host)),
            (":".to_string(), style(palette.prompt_host)),
            (self.path.clone(), style(palette.prompt_path)),
            (format!("{} ", self.symbol), style(palette.prompt_symbol)),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorConfig {
    pub style: CursorStyle,
    pub blink: Option<Time>,
    pub visible: bool,
}

impl Default for CursorConfig {
    fn default() -> Self {
        CursorConfig {
            style: CursorStyle::Block,
            blink: Some(DEFAULT_CURSOR_BLINK),
            visible: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlaybackConfig {
    pub looping: bool,
    pub loop_delay: Time,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    WebM,
    Mp4,
    Gif,
    Png,
}

impl OutputFormat {
    /// H.264 requires even width and height; this is validated before rendering
    /// starts rather than failing partway through an encode.
    pub fn requires_even_dimensions(self) -> bool {
        matches!(self, OutputFormat::Mp4)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OutputFormat::WebM => "webm",
            OutputFormat::Mp4 => "mp4",
            OutputFormat::Gif => "gif",
            OutputFormat::Png => "png",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Vp9,
    H264,
}

/// Default CRF-style quality for VP9/H.264 encoding. 32 sits in the
/// "visually lossless enough" range for typing-animation content without the
/// file size cost of pushing toward 0.
pub const DEFAULT_QUALITY: u8 = 32;

/// Highest valid `-crf` value FFmpeg accepts for `libvpx-vp9`. 0 is
/// the highest quality; values above this are rejected by the encoder itself,
/// so scenarios are validated against it before a render ever spawns FFmpeg.
pub const MAX_QUALITY_VP9: u8 = 63;

/// Highest valid `-crf` value FFmpeg accepts for `libx264`.
pub const MAX_QUALITY_H264: u8 = 51;

#[derive(Debug, Clone, PartialEq)]
pub struct OutputConfig {
    pub format: OutputFormat,
    pub codec: Option<Codec>,
    pub path: PathBuf,
    pub size: Size,
    pub fps: Fps,
    pub transparent: bool,
    /// CRF for VP9/H.264. Lower is higher quality.
    pub quality: u8,
    pub overwrite: bool,
}

/// Fully resolved theme colors. Names mirror the reference palette's naming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub background: Color,
    pub foreground: Color,
    pub prompt_user: Color,
    pub prompt_host: Color,
    pub prompt_path: Color,
    pub prompt_symbol: Color,
    pub command: Color,
    pub cursor: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub muted: Color,
}

impl Default for Palette {
    /// The `zombocoder` reference palette.
    fn default() -> Self {
        Palette {
            background: Color::rgb(0x08, 0x0B, 0x09),
            foreground: Color::rgb(0xC8, 0xE6, 0xC9),
            prompt_user: Color::rgb(0x39, 0xFF, 0x14),
            prompt_host: Color::rgb(0x69, 0xD2, 0xFF),
            prompt_path: Color::rgb(0xA7, 0xB0, 0xA7),
            prompt_symbol: Color::rgb(0x39, 0xFF, 0x14),
            command: Color::rgb(0xFF, 0xFF, 0xFF),
            cursor: Color::rgb(0x39, 0xFF, 0x14),
            success: Color::rgb(0x39, 0xFF, 0x14),
            warning: Color::rgb(0xF1, 0xC4, 0x0F),
            error: Color::rgb(0xFF, 0x4D, 0x4D),
            muted: Color::rgb(0x66, 0x72, 0x66),
        }
    }
}

/// Terminal grid dimensions in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSpec {
    pub cols: u16,
    pub rows: u16,
}

/// JetBrains Mono's monospace advance width as a fraction of em (font size).
/// The two agree for the default font, which is why this stayed unnoticed as
/// the estimate used by `validate`/`inspect` for other fonts: it's a rough
/// approximation, not a substitute for a real shaped advance.
const MONOSPACE_ADVANCE_RATIO: f32 = 0.6;

impl GridSpec {
    /// Estimates the grid from font metrics without loading a font file.
    ///
    /// This is a font-free approximation only: it does not agree with the
    /// real shaped advance for most fonts (`MONOSPACE_ADVANCE_RATIO`'s doc
    /// comment explains why). `termotion_render::grid_for`, which shapes the
    /// actual font, is what the renderer uses and what `validate` and
    /// `inspect` should also use so they report the grid that will actually
    /// render. Kept only for callers that must derive a grid without paying
    /// for a font load.
    pub fn estimate(font: &FontConfig, terminal: &TerminalConfig) -> GridSpec {
        let advance = (font.size * MONOSPACE_ADVANCE_RATIO).round().max(1.0) as u32;
        let line_h = (font.size * font.line_height).round().max(1.0) as u32;
        GridSpec::from_metrics(advance, line_h, terminal)
    }

    pub fn from_metrics(advance_w: u32, line_h: u32, terminal: &TerminalConfig) -> GridSpec {
        let inner = terminal.bounds.inset(terminal.padding);
        GridSpec {
            cols: u16::try_from((inner.width / advance_w).max(1)).unwrap_or(u16::MAX),
            rows: u16::try_from((inner.height / line_h).max(1)).unwrap_or(u16::MAX),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_renders_the_documented_form() {
        let prompt = PromptConfig {
            user: "zombocoder".into(),
            host: "twitch".into(),
            path: "~".into(),
            symbol: "$".into(),
        };
        let palette = Palette::default();
        let spans = prompt.spans(&palette);
        let text: String = spans.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(text, "zombocoder@twitch:~$ ");
    }

    #[test]
    fn prompt_segments_carry_distinct_palette_colors() {
        let prompt = PromptConfig {
            user: "zombocoder".into(),
            host: "twitch".into(),
            path: "~".into(),
            symbol: "$".into(),
        };
        let palette = Palette {
            prompt_user: Color::rgb(57, 255, 20),
            prompt_host: Color::rgb(105, 210, 255),
            prompt_path: Color::rgb(167, 176, 167),
            ..Palette::default()
        };
        let spans = prompt.spans(&palette);
        assert_eq!(spans[0].1.fg, Color::rgb(57, 255, 20)); // user
        assert_eq!(spans[2].1.fg, Color::rgb(105, 210, 255)); // host
        assert_eq!(spans[4].1.fg, Color::rgb(167, 176, 167)); // path
    }

    #[test]
    fn root_symbol_is_configurable() {
        let prompt = PromptConfig {
            user: "root".into(),
            host: "box".into(),
            path: "/".into(),
            symbol: "#".into(),
        };
        let text: String = prompt
            .spans(&Palette::default())
            .iter()
            .map(|(t, _)| t.as_str())
            .collect();
        assert_eq!(text, "root@box:/# ");
    }

    #[test]
    fn canvas_defaults_match_the_spec() {
        let canvas = CanvasConfig::default();
        assert_eq!(canvas.size.width, 1920);
        assert_eq!(canvas.size.height, 1080);
        assert_eq!(canvas.fps, Fps::from_integer(30));
        assert!(!canvas.transparent);
    }

    #[test]
    fn h264_rejects_odd_dimensions() {
        assert!(OutputFormat::Mp4.requires_even_dimensions());
        assert!(!OutputFormat::Png.requires_even_dimensions());
        assert!(!OutputFormat::WebM.requires_even_dimensions());
    }
}
