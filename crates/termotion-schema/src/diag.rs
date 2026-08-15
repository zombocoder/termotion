use std::fmt;
use std::path::{Path, PathBuf};

/// The error-code registry. Codes are stable and documented; never renumber one.
pub mod codes {
    /// `(code, exit_code)` pairs. Exit codes follow the documented exit-code table.
    pub type Code = (&'static str, i32);

    pub const YAML_SYNTAX: Code = ("E000", 3);
    pub const UNSUPPORTED_VERSION: Code = ("E001", 4);
    pub const MISSING_FIELD: Code = ("E002", 4);
    pub const UNKNOWN_FIELD: Code = ("E003", 4);
    pub const UNKNOWN_ACTION: Code = ("E004", 4);
    pub const BAD_DURATION: Code = ("E005", 4);
    pub const BAD_COLOR: Code = ("E006", 4);
    pub const UNDEFINED_VARIABLE: Code = ("E007", 4);
    pub const INCLUDE_CYCLE: Code = ("E008", 4);
    pub const INCLUDE_NOT_FOUND: Code = ("E009", 5);
    pub const INCLUDE_TOO_DEEP: Code = ("E010", 4);
    pub const UNKNOWN_THEME: Code = ("E011", 4);
    pub const BAD_DIMENSION: Code = ("E012", 4);
    pub const ODD_DIMENSION_FOR_H264: Code = ("E013", 4);
    pub const DURATION_NOT_POSITIVE: Code = ("E014", 4);
    pub const TERMINAL_OUT_OF_BOUNDS: Code = ("E015", 4);
    pub const OVERFLOW_ERROR_MODE: Code = ("E016", 4);
    /// `output.quality` outside the encoder's valid `-crf` range. Caught here
    /// rather than left to fail deep inside FFmpeg as an opaque `ENCODER_FAILED`.
    pub const BAD_QUALITY: Code = ("E017", 4);
    /// `output.path`'s filename starts with `-`, which FFmpeg's argument
    /// parser would read as an option rather than a positional output path.
    pub const BAD_OUTPUT_PATH: Code = ("E018", 4);
    pub const FONT_NOT_FOUND: Code = ("E020", 5);
    pub const OUTPUT_EXISTS: Code = ("E030", 1);
    pub const FFMPEG_NOT_FOUND: Code = ("E040", 7);
    pub const ENCODER_MISSING: Code = ("E041", 7);
    pub const ENCODER_FAILED: Code = ("E042", 7);
    /// A `termotion-render` failure that is not a font problem (`FONT_NOT_FOUND`
    /// already owns that case) or a size-mismatch bug — kept distinct from
    /// `ENCODER_FAILED`'s exit code 7 so the two pipeline stages are
    /// distinguishable from the exit code alone.
    pub const RENDER_FAILED: Code = ("E050", 6);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: codes::Code,
    pub message: String,
    pub path: Option<String>,
    pub position: Option<Position>,
    pub file: Option<PathBuf>,
    pub hint: Option<String>,
}

impl Diagnostic {
    pub fn error(code: codes::Code, message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Error,
            code,
            message: message.into(),
            path: None,
            position: None,
            file: None,
            hint: None,
        }
    }

    #[must_use]
    pub fn at_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    #[must_use]
    pub fn at(mut self, position: Position) -> Self {
        self.position = Some(position);
        self
    }

    #[must_use]
    pub fn at_opt(mut self, position: Option<Position>) -> Self {
        self.position = position;
        self
    }

    #[must_use]
    pub fn in_file(mut self, file: impl AsRef<Path>) -> Self {
        self.file = Some(file.as_ref().to_path_buf());
        self
    }

    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn exit_code(&self) -> i32 {
        self.code.1
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        writeln!(f, "{label}[{}]:", self.code.0)?;
        writeln!(f, "{}", self.message)?;

        if let (Some(file), Some(pos)) = (&self.file, self.position) {
            writeln!(f)?;
            writeln!(f, " --> {}:{}:{}", file.display(), pos.line, pos.column)?;
        }
        if let Some(hint) = &self.hint {
            writeln!(f)?;
            writeln!(f, "hint:")?;
            writeln!(f, "{hint}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostic {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_render_the_documented_format() {
        let diag = Diagnostic::error(
            codes::DURATION_NOT_POSITIVE,
            "timeline[4].duration must be greater than 0",
        )
        .at_path("timeline[4].duration")
        .at(Position {
            line: 28,
            column: 15,
        })
        .in_file("scenes/brb.yaml");

        let rendered = diag.to_string();
        assert!(rendered.contains("error[E014]:"), "got: {rendered}");
        assert!(rendered.contains("timeline[4].duration must be greater than 0"));
        assert!(
            rendered.contains("--> scenes/brb.yaml:28:15"),
            "got: {rendered}"
        );
    }

    #[test]
    fn hints_are_rendered_when_present() {
        let diag = Diagnostic::error(codes::FONT_NOT_FOUND, "font \"JetBrains Mono\" not found")
            .with_hint("Install the font or specify:\n\nfont:\n  path: ./assets/fonts/font.ttf");
        let rendered = diag.to_string();
        assert!(rendered.contains("hint:"));
        assert!(rendered.contains("path: ./assets/fonts/font.ttf"));
    }

    #[test]
    fn diagnostics_without_a_location_still_render() {
        let rendered =
            Diagnostic::error(codes::UNSUPPORTED_VERSION, "unsupported version").to_string();
        assert!(rendered.contains("error[E001]:"));
        assert!(!rendered.contains("-->"));
    }

    #[test]
    fn error_codes_map_to_spec_exit_codes() {
        assert_eq!(Diagnostic::error(codes::YAML_SYNTAX, "x").exit_code(), 3);
        assert_eq!(
            Diagnostic::error(codes::DURATION_NOT_POSITIVE, "x").exit_code(),
            4
        );
        assert_eq!(Diagnostic::error(codes::FONT_NOT_FOUND, "x").exit_code(), 5);
    }
}
