pub mod color;
pub mod config;
pub mod frame;
pub mod geom;
pub mod grid;
pub mod scenario;
pub mod style;
pub mod terminal;
pub mod time;

pub use color::{parse_color, Color, ColorParseError};
pub use config::{
    BackgroundConfig, CanvasConfig, Codec, CursorConfig, FontConfig, GridSpec, OutputConfig,
    OutputFormat, Palette, PlaybackConfig, PromptConfig, TerminalConfig, DEFAULT_QUALITY,
    DEFAULT_TERMINAL_MARGIN, MAX_CANVAS_DIMENSION, MAX_QUALITY_H264, MAX_QUALITY_VP9,
};
pub use frame::Frame;
pub use geom::{Padding, Point, Rect, Size};
pub use grid::{Cell, Grid, Overflow, Row};
pub use scenario::{Action, ClearEffect, Metadata, Scenario, TextRun};
pub use style::{StyleId, StyleTable, TextStyle};
pub use terminal::{CursorState, CursorStyle, TerminalState, DEFAULT_CURSOR_BLINK};
pub use time::{parse_duration, DurationParseError, Fps, Time};
