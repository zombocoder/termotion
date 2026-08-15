use crate::color::Color;
use crate::config::{
    CanvasConfig, CursorConfig, FontConfig, Palette, PlaybackConfig, PromptConfig, TerminalConfig,
};
use crate::time::Time;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub name: String,
    pub description: Option<String>,
}

/// A scenario after resolution: includes merged, variables substituted, theme
/// flattened to a palette, durations parsed, colors concrete.
#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    pub metadata: Metadata,
    pub canvas: CanvasConfig,
    pub terminal: TerminalConfig,
    pub font: FontConfig,
    pub prompt: PromptConfig,
    pub cursor: CursorConfig,
    pub palette: Palette,
    pub playback: PlaybackConfig,
    pub seed: u64,
    pub timeline: Vec<Action>,
}

/// Timeline actions available in M1–M4.
///
/// `status`, `status_update`, `progress`, `spinner`, `glitch`, and `flash` are M5/M6
/// and are deliberately absent — adding a variant here without a compiler arm would
/// be a silent no-op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Write {
        spans: Vec<TextRun>,
        speed: Time,
    },
    WriteLine {
        spans: Vec<TextRun>,
        speed: Time,
    },
    Command {
        text: String,
        speed: Time,
        enter_delay: Time,
    },
    Pause {
        duration: Time,
    },
    PauseRandom {
        min: Time,
        max: Time,
    },
    Newline {
        count: u32,
    },
    Clear {
        effect: ClearEffect,
    },
    Backspace {
        count: u32,
        speed: Time,
    },
    Cursor {
        visible: bool,
        blink: Option<Time>,
        duration: Time,
    },
    SetColor {
        foreground: Option<Color>,
        background: Option<Color>,
    },
    ResetStyle,
}

/// One styled run within a `write`. A plain `text:` write produces exactly one run
/// with `color: None`, meaning "use the current style".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRun {
    pub text: String,
    pub color: Option<Color>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClearEffect {
    #[default]
    Instant,
    Terminal,
}
