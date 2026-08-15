use std::collections::BTreeMap;

use serde::Deserialize;

use crate::diag::{codes, Diagnostic, Position};

pub const SUPPORTED_VERSION: u32 = 1;

/// Probes only the version field, so an unsupported version is reported before
/// unknown-field errors from a future schema drown it out.
#[derive(Debug, Deserialize)]
struct VersionProbe {
    version: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RawScenario {
    pub version: u32,
    #[serde(default)]
    pub metadata: Option<RawMetadata>,
    #[serde(default)]
    pub includes: Vec<String>,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    #[serde(default)]
    pub canvas: Option<RawCanvas>,
    #[serde(default)]
    pub theme: Option<RawThemeRef>,
    #[serde(default)]
    pub terminal: Option<RawTerminal>,
    #[serde(default)]
    pub font: Option<RawFont>,
    #[serde(default)]
    pub prompt: Option<RawPrompt>,
    #[serde(default)]
    pub cursor: Option<RawCursor>,
    #[serde(default)]
    pub playback: Option<RawPlayback>,
    #[serde(default)]
    pub output: Option<RawOutput>,
    #[serde(default)]
    pub random: Option<RawRandom>,
    #[serde(default)]
    pub timeline: Vec<RawAction>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RawMetadata {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RawCanvas {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    pub background: Option<String>,
    pub transparent: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum RawThemeRef {
    Named(String),
    Ref {
        #[serde(rename = "ref")]
        reference: String,
    },
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RawTerminal {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub padding: Option<RawPadding>,
    pub overflow: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RawPadding {
    pub top: Option<u32>,
    pub right: Option<u32>,
    pub bottom: Option<u32>,
    pub left: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct RawFont {
    pub family: Option<String>,
    pub path: Option<String>,
    pub size: Option<f32>,
    pub weight: Option<u16>,
    pub line_height: Option<f32>,
    pub letter_spacing: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RawPrompt {
    pub user: Option<String>,
    pub host: Option<String>,
    pub path: Option<String>,
    pub symbol: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RawCursor {
    pub style: Option<String>,
    pub blink: Option<String>,
    pub visible: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RawPlayback {
    #[serde(rename = "loop")]
    pub looping: Option<bool>,
    pub loop_delay: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RawOutput {
    pub format: Option<String>,
    pub codec: Option<String>,
    pub quality: Option<u8>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RawRandom {
    pub seed: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RawSpan {
    pub text: String,
    #[serde(default)]
    pub color: Option<String>,
}

/// Timeline actions as written in YAML. M5/M6 actions are intentionally absent —
/// see `unknown_action_hint` for the message users get if they try one early.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RawAction {
    Write {
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        spans: Option<Vec<RawSpan>>,
        #[serde(default)]
        speed: Option<String>,
    },
    WriteLine {
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        spans: Option<Vec<RawSpan>>,
        #[serde(default)]
        speed: Option<String>,
    },
    Command {
        text: String,
        #[serde(default)]
        speed: Option<String>,
        #[serde(default)]
        enter_delay: Option<String>,
    },
    Pause {
        duration: String,
    },
    PauseRandom {
        min: String,
        max: String,
        #[serde(default)]
        seed: Option<u64>,
    },
    Newline {
        #[serde(default)]
        count: Option<u32>,
    },
    Clear {
        #[serde(default)]
        effect: Option<String>,
    },
    Backspace {
        #[serde(default)]
        count: Option<u32>,
        #[serde(default)]
        speed: Option<String>,
    },
    Cursor {
        #[serde(default)]
        visible: Option<bool>,
        #[serde(default)]
        blink: Option<String>,
        #[serde(default)]
        duration: Option<String>,
        #[serde(default)]
        style: Option<String>,
    },
    SetColor {
        #[serde(default)]
        foreground: Option<String>,
        #[serde(default)]
        background: Option<String>,
    },
    ResetStyle,
}

/// Actions that exist in the product spec but are not implemented until M5/M6.
const DEFERRED_ACTIONS: &[&str] = &[
    "status",
    "status_update",
    "progress",
    "spinner",
    "glitch",
    "flash",
];

pub fn parse(source: &str) -> Result<RawScenario, Diagnostic> {
    // 1. Version gate first.
    let probe: VersionProbe = serde_yaml_ng::from_str(source).map_err(yaml_diagnostic)?;
    match probe.version {
        None => {
            return Err(Diagnostic::error(
                codes::MISSING_FIELD,
                "scenario is missing the required `version` field",
            )
            .at_path("version")
            .with_hint("Add `version: 1` as the first line of the scenario."))
        }
        Some(v) if v != SUPPORTED_VERSION => {
            return Err(Diagnostic::error(
                codes::UNSUPPORTED_VERSION,
                format!("scenario version {v} is not supported (this build understands version {SUPPORTED_VERSION})"),
            )
            .at_path("version"))
        }
        Some(_) => {}
    }

    // 2. Full parse.
    serde_yaml_ng::from_str(source).map_err(|err| classify(err, source))
}

fn yaml_diagnostic(err: serde_yaml_ng::Error) -> Diagnostic {
    Diagnostic::error(codes::YAML_SYNTAX, format!("invalid YAML: {err}")).at_opt(position_of(&err))
}

fn position_of(err: &serde_yaml_ng::Error) -> Option<Position> {
    err.location().map(|loc| Position {
        line: loc.line() as u32,
        column: loc.column() as u32,
    })
}

/// Turns a serde error into the most specific diagnostic we can justify from its
/// message, so users get `E004 unknown action` rather than a generic parse failure.
fn classify(err: serde_yaml_ng::Error, _source: &str) -> Diagnostic {
    let text = err.to_string();
    let position = position_of(&err);

    if text.contains("unknown variant") {
        let named = DEFERRED_ACTIONS.iter().find(|name| text.contains(**name));
        let mut diag = Diagnostic::error(
            codes::UNKNOWN_ACTION,
            format!("unknown action type: {text}"),
        )
        .at_opt(position);
        if let Some(name) = named {
            diag = diag.with_hint(format!(
                "`{name}` is defined in the product spec but is not implemented in this build yet."
            ));
        }
        return diag;
    }
    if text.contains("unknown field") {
        return Diagnostic::error(codes::UNKNOWN_FIELD, text).at_opt(position);
    }
    if text.contains("missing field") {
        return Diagnostic::error(codes::MISSING_FIELD, text).at_opt(position);
    }
    Diagnostic::error(codes::YAML_SYNTAX, format!("invalid YAML: {text}")).at_opt(position)
}

/// Deserializes an already-merged, already-substituted value tree.
///
/// `parse` is the convenience path for sources with no includes or variables;
/// the full loader goes through the value tree and calls this instead.
pub fn from_value(value: serde_yaml_ng::Value) -> Result<RawScenario, Diagnostic> {
    let version = value.get("version").and_then(serde_yaml_ng::Value::as_u64);
    match version {
        None => {
            return Err(Diagnostic::error(
                codes::MISSING_FIELD,
                "scenario is missing the required `version` field",
            )
            .at_path("version")
            .with_hint("Add `version: 1` as the first line of the scenario."))
        }
        Some(v) if v != u64::from(SUPPORTED_VERSION) => {
            return Err(Diagnostic::error(
                codes::UNSUPPORTED_VERSION,
                format!("scenario version {v} is not supported (this build understands version {SUPPORTED_VERSION})"),
            )
            .at_path("version"))
        }
        Some(_) => {}
    }

    serde_yaml_ng::from_value(value).map_err(|err| classify(err, ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_scenario() {
        let raw = parse("version: 1\ntimeline: []\n").unwrap();
        assert_eq!(raw.version, 1);
        assert!(raw.timeline.is_empty());
    }

    #[test]
    fn rejects_unsupported_versions_before_anything_else() {
        // Version 2 uses fields we do not understand; the version error must win.
        let err = parse("version: 2\ntimeline:\n  - type: nonsense\n").unwrap_err();
        assert_eq!(err.code, codes::UNSUPPORTED_VERSION);
        assert!(err.message.contains('2'));
    }

    #[test]
    fn requires_a_version_field() {
        let err = parse("timeline: []\n").unwrap_err();
        assert_eq!(err.code, codes::MISSING_FIELD);
    }

    #[test]
    fn parses_every_m1_m4_action() {
        let src = "\
version: 1
timeline:
  - type: command
    text: ./brb
    speed: 45ms
    enter_delay: 250ms
  - type: pause
    duration: 500ms
  - type: write
    text: Session suspended.
  - type: write_line
    text: 'Reason:'
  - type: newline
    count: 2
  - type: backspace
    count: 5
    speed: 40ms
  - type: clear
    effect: terminal
  - type: cursor
    visible: true
    blink: 500ms
    duration: 5s
  - type: set_color
    foreground: '#FFFFFF'
  - type: reset_style
  - type: pause_random
    min: 300ms
    max: 600ms
";
        let raw = parse(src).unwrap();
        assert_eq!(raw.timeline.len(), 11);
        assert!(matches!(raw.timeline[0], RawAction::Command { .. }));
        assert!(matches!(raw.timeline[10], RawAction::PauseRandom { .. }));
    }

    #[test]
    fn parses_write_with_spans() {
        let src = "\
version: 1
timeline:
  - type: write
    spans:
      - text: 'Reason:'
        color: foreground
      - text: ' coffee'
        color: warning
";
        let raw = parse(src).unwrap();
        match &raw.timeline[0] {
            RawAction::Write { spans, .. } => {
                let spans = spans.as_ref().expect("spans present");
                assert_eq!(spans.len(), 2);
                assert_eq!(spans[1].color.as_deref(), Some("warning"));
            }
            other => panic!("expected write, got {other:?}"),
        }
    }

    #[test]
    fn rejects_unknown_action_types() {
        let err = parse("version: 1\ntimeline:\n  - type: teleport\n").unwrap_err();
        assert_eq!(err.code, codes::UNKNOWN_ACTION);
        assert!(err.message.contains("teleport"));
    }

    #[test]
    fn rejects_unknown_fields() {
        let err = parse("version: 1\ncanvas:\n  widht: 1920\n").unwrap_err();
        assert_eq!(err.code, codes::UNKNOWN_FIELD);
    }

    #[test]
    fn m5_actions_are_rejected_with_a_helpful_message() {
        let err = parse("version: 1\ntimeline:\n  - type: status\n    text: camera\n").unwrap_err();
        assert_eq!(err.code, codes::UNKNOWN_ACTION);
        assert!(err.message.contains("status"));
    }

    #[test]
    fn syntax_errors_carry_a_position() {
        let err = parse("version: 1\ncanvas:\n  - [unclosed\n").unwrap_err();
        assert_eq!(err.code, codes::YAML_SYNTAX);
        assert!(err.position.is_some());
    }
}
