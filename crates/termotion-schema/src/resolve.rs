use std::path::PathBuf;

use termotion_core::{
    parse_color, parse_duration, Action, BackgroundConfig, CanvasConfig, ClearEffect, Codec, Color,
    CursorConfig, CursorStyle, FontConfig, Fps, Metadata, OutputConfig, OutputFormat, Overflow,
    Padding, Palette, PlaybackConfig, PromptConfig, Rect, Scenario, Size, TerminalConfig, TextRun,
    Time, DEFAULT_CURSOR_BLINK, DEFAULT_QUALITY, DEFAULT_TERMINAL_MARGIN, MAX_CANVAS_DIMENSION,
    MAX_QUALITY_H264, MAX_QUALITY_VP9,
};

use crate::diag::{codes, Diagnostic};
use crate::project::ProjectConfig;
use crate::raw::{RawAction, RawScenario, RawSpan, RawThemeRef};
use crate::theme;

/// Every field the CLI can override (the highest-priority layer).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    pub theme: Option<String>,
    pub background: Option<String>,
    pub transparent: Option<bool>,
    pub looping: Option<bool>,
    pub quality: Option<u8>,
    pub format: Option<String>,
    pub output: Option<PathBuf>,
    pub overwrite: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ResolveContext {
    pub overrides: Overrides,
    pub project: Option<ProjectConfig>,
    pub project_root: Option<PathBuf>,
    /// Directory of the scenario file, used to resolve relative font paths.
    pub scenario_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub scenario: Scenario,
    pub output: OutputConfig,
}

pub fn resolve(raw: RawScenario, ctx: ResolveContext) -> Result<Loaded, Vec<Diagnostic>> {
    let mut errors: Vec<Diagnostic> = Vec::new();

    let defaults = ctx.project.as_ref().and_then(|p| p.defaults.clone());
    let ov = &ctx.overrides;

    // ---- canvas -----------------------------------------------------------
    let raw_canvas = raw.canvas.clone().unwrap_or_default();
    let base = CanvasConfig::default();

    let width = ov
        .width
        .or(raw_canvas.width)
        .or(defaults.as_ref().and_then(|d| d.width))
        .unwrap_or(base.size.width);
    let height = ov
        .height
        .or(raw_canvas.height)
        .or(defaults.as_ref().and_then(|d| d.height))
        .unwrap_or(base.size.height);
    let fps_value = ov
        .fps
        .or(raw_canvas.fps)
        .or(defaults.as_ref().and_then(|d| d.fps))
        .unwrap_or(base.fps.num());

    if width == 0 || height == 0 {
        errors.push(
            Diagnostic::error(
                codes::BAD_DIMENSION,
                "canvas width and height must be greater than 0",
            )
            .at_path("canvas.width"),
        );
    }
    // Bounding the canvas here is what keeps `validate` and `render` from
    // disagreeing about whether a scenario is legal: an unbounded `u32`
    // dimension (e.g. `width: 4294967295`) previously passed validation and
    // then made `render` attempt a multi-exabyte frame-buffer allocation,
    // killed by the OS with no diagnostic at all. The terminal region does
    // not need its own check: it is already required to fit inside the
    // canvas (below), so bounding the canvas transitively bounds it too.
    if width > MAX_CANVAS_DIMENSION || height > MAX_CANVAS_DIMENSION {
        errors.push(
            Diagnostic::error(
                codes::BAD_DIMENSION,
                format!(
                    "canvas width and height must not exceed {MAX_CANVAS_DIMENSION} pixels each (got {width}x{height})"
                ),
            )
            .at_path("canvas.width")
            .with_hint(format!(
                "{MAX_CANVAS_DIMENSION} pixels comfortably exceeds 8K (7680x4320); \
                 use a smaller canvas."
            )),
        );
    }
    let fps = match Fps::new(fps_value, 1) {
        Some(fps) => fps,
        None => {
            errors.push(
                Diagnostic::error(codes::BAD_DIMENSION, "canvas fps must be greater than 0")
                    .at_path("canvas.fps"),
            );
            base.fps
        }
    };

    let transparent = ov.transparent.or(raw_canvas.transparent).unwrap_or(false);

    // ---- palette ----------------------------------------------------------
    let theme_name = ov
        .theme
        .clone()
        .or_else(|| match raw.theme.clone() {
            Some(RawThemeRef::Named(name)) => Some(name),
            Some(RawThemeRef::Ref { reference }) => Some(reference),
            None => None,
        })
        .or_else(|| defaults.as_ref().and_then(|d| d.theme.clone()));

    let theme_dirs: Vec<PathBuf> = match (&ctx.project, &ctx.project_root) {
        (Some(project), Some(root)) => vec![project.themes_dir(root)],
        _ => Vec::new(),
    };

    let palette = match &theme_name {
        Some(name) => match theme::load_theme(name, &theme_dirs) {
            Ok(palette) => palette,
            Err(diag) => {
                errors.push(diag.at_path("theme"));
                Palette::default()
            }
        },
        None => Palette::default(),
    };

    let background_source = ov.background.clone().or(raw_canvas.background.clone());
    let background_color = match &background_source {
        Some(text) => match parse_color(text) {
            Ok(color) => color,
            Err(err) => {
                errors.push(
                    Diagnostic::error(codes::BAD_COLOR, err.to_string())
                        .at_path("canvas.background"),
                );
                palette.background
            }
        },
        None => palette.background,
    };

    // ---- terminal ---------------------------------------------------------
    let raw_terminal = raw.terminal.clone().unwrap_or_default();
    let term_default = TerminalConfig::default();
    let bounds = Rect {
        x: raw_terminal.x.unwrap_or(term_default.bounds.x),
        y: raw_terminal.y.unwrap_or(term_default.bounds.y),
        width: raw_terminal
            .width
            .unwrap_or_else(|| width.saturating_sub(2 * DEFAULT_TERMINAL_MARGIN).max(1)),
        height: raw_terminal
            .height
            .unwrap_or_else(|| height.saturating_sub(2 * DEFAULT_TERMINAL_MARGIN).max(1)),
    };

    if bounds.x < 0
        || bounds.y < 0
        || bounds.x as i64 + i64::from(bounds.width) > i64::from(width)
        || bounds.y as i64 + i64::from(bounds.height) > i64::from(height)
    {
        errors.push(
            Diagnostic::error(
                codes::TERMINAL_OUT_OF_BOUNDS,
                format!(
                    "terminal region {}x{} at ({},{}) does not fit inside the {}x{} canvas",
                    bounds.width, bounds.height, bounds.x, bounds.y, width, height
                ),
            )
            .at_path("terminal"),
        );
    }

    let padding = raw_terminal
        .padding
        .as_ref()
        .map(|p| Padding {
            top: p.top.unwrap_or(0),
            right: p.right.unwrap_or(0),
            bottom: p.bottom.unwrap_or(0),
            left: p.left.unwrap_or(0),
        })
        .unwrap_or(term_default.padding);

    let overflow = match raw_terminal.overflow.as_deref() {
        None => Overflow::Scroll,
        Some("scroll") => Overflow::Scroll,
        Some("clip") => Overflow::Clip,
        Some("error") => Overflow::Error,
        Some(other) => {
            errors.push(
                Diagnostic::error(
                    codes::UNKNOWN_FIELD,
                    format!("unknown overflow mode `{other}` (expected clip, scroll, or error)"),
                )
                .at_path("terminal.overflow"),
            );
            Overflow::Scroll
        }
    };

    // ---- font -------------------------------------------------------------
    let raw_font = raw.font.clone().unwrap_or_default();
    let font_base = FontConfig::default();
    let font = FontConfig {
        family: raw_font.family.unwrap_or(font_base.family),
        path: raw_font.path.map(|p| match &ctx.scenario_dir {
            Some(dir) => dir.join(p),
            None => PathBuf::from(p),
        }),
        size: raw_font.size.unwrap_or(font_base.size),
        weight: raw_font.weight.unwrap_or(font_base.weight),
        line_height: raw_font.line_height.unwrap_or(font_base.line_height),
        letter_spacing: raw_font.letter_spacing.unwrap_or(font_base.letter_spacing),
    };
    if font.size <= 0.0 {
        errors.push(
            Diagnostic::error(codes::BAD_DIMENSION, "font size must be greater than 0")
                .at_path("font.size"),
        );
    }

    // ---- prompt and cursor -------------------------------------------------
    let raw_prompt = raw.prompt.clone().unwrap_or_default();
    let prompt_base = PromptConfig::default();
    let prompt = PromptConfig {
        user: raw_prompt.user.unwrap_or(prompt_base.user),
        host: raw_prompt.host.unwrap_or(prompt_base.host),
        path: raw_prompt.path.unwrap_or(prompt_base.path),
        symbol: raw_prompt.symbol.unwrap_or(prompt_base.symbol),
    };

    let raw_cursor = raw.cursor.clone().unwrap_or_default();
    let cursor = CursorConfig {
        style: match raw_cursor.style.as_deref() {
            None | Some("block") => CursorStyle::Block,
            Some("underline") => CursorStyle::Underline,
            Some("bar") => CursorStyle::Bar,
            Some(other) => {
                errors.push(
                    Diagnostic::error(
                        codes::UNKNOWN_FIELD,
                        format!(
                            "unknown cursor style `{other}` (expected block, underline, or bar)"
                        ),
                    )
                    .at_path("cursor.style"),
                );
                CursorStyle::Block
            }
        },
        blink: match &raw_cursor.blink {
            None => Some(DEFAULT_CURSOR_BLINK),
            Some(text) => match parse_duration(text) {
                Ok(t) if t == Time::ZERO => None,
                Ok(t) => Some(t),
                Err(err) => {
                    errors.push(
                        Diagnostic::error(codes::BAD_DURATION, err.to_string())
                            .at_path("cursor.blink"),
                    );
                    Some(DEFAULT_CURSOR_BLINK)
                }
            },
        },
        visible: raw_cursor.visible.unwrap_or(true),
    };

    // ---- playback ----------------------------------------------------------
    let raw_playback = raw.playback.clone().unwrap_or_default();
    let playback = PlaybackConfig {
        looping: ov.looping.or(raw_playback.looping).unwrap_or(false),
        loop_delay: raw_playback
            .loop_delay
            .as_deref()
            .map(|text| duration_or_zero(text, "playback.loop_delay", &mut errors))
            .unwrap_or(Time::ZERO),
    };

    // ---- timeline ----------------------------------------------------------
    let timeline = raw
        .timeline
        .iter()
        .enumerate()
        .map(|(i, action)| resolve_action(i, action, &palette, &mut errors))
        .collect::<Vec<_>>();

    // ---- output ------------------------------------------------------------
    let raw_output = raw.output.clone().unwrap_or_default();
    let format_name = ov.format.clone().or(raw_output.format.clone());
    let format = match format_name.as_deref() {
        None => OutputFormat::WebM,
        Some("webm") => OutputFormat::WebM,
        Some("mp4") => OutputFormat::Mp4,
        Some("gif") => OutputFormat::Gif,
        Some("png") => OutputFormat::Png,
        Some(other) => {
            errors.push(
                Diagnostic::error(
                    codes::UNKNOWN_FIELD,
                    format!("unknown output format `{other}` (expected webm, mp4, gif, or png)"),
                )
                .at_path("output.format"),
            );
            OutputFormat::WebM
        }
    };

    if format.requires_even_dimensions() && (width % 2 == 1 || height % 2 == 1) {
        errors.push(
            Diagnostic::error(
                codes::ODD_DIMENSION_FOR_H264,
                format!("H.264 requires even dimensions; canvas is {width}x{height}"),
            )
            .at_path("canvas.width")
            .with_hint("Round the canvas size to even numbers, or render to WebM instead."),
        );
    }

    let codec = match raw_output.codec.as_deref() {
        None => None,
        Some("vp9") => Some(Codec::Vp9),
        Some("h264") => Some(Codec::H264),
        Some(other) => {
            errors.push(
                Diagnostic::error(
                    codes::UNKNOWN_FIELD,
                    format!("unknown codec `{other}` (expected vp9 or h264)"),
                )
                .at_path("output.codec"),
            );
            None
        }
    };

    let name = raw
        .metadata
        .as_ref()
        .and_then(|m| m.name.clone())
        .unwrap_or_else(|| "untitled".to_string());

    let output_dir = match (&ctx.project, &ctx.project_root) {
        (Some(project), Some(root)) => project.output_dir(root),
        _ => PathBuf::from("."),
    };
    let output_path = ov
        .output
        .clone()
        .or_else(|| raw_output.path.clone().map(PathBuf::from))
        .unwrap_or_else(|| output_dir.join(format!("{name}.{}", format.as_str())));

    // `output.path` is scenario/CLI-controlled data that ends up as FFmpeg's
    // last positional argument (see `termotion_encode::ffmpeg`). A filename
    // starting with `-` would be read by FFmpeg's argument parser as an
    // option rather than an output path (e.g. `-loglevel`), so it is rejected
    // here rather than left to surface as a confusing FFmpeg error deep in
    // the encoder — or worse, silently change FFmpeg's behavior.
    if let Some(file_name) = output_path.file_name().and_then(|n| n.to_str()) {
        if file_name.starts_with('-') {
            errors.push(
                Diagnostic::error(
                    codes::BAD_OUTPUT_PATH,
                    format!(
                        "output.path `{}` starts with `-`, which FFmpeg would read as an option rather than a file path",
                        output_path.display()
                    ),
                )
                .at_path("output.path")
                .with_hint("Rename the file so it does not start with `-`, or prefix the path with `./`."),
            );
        }
    }

    let quality = ov.quality.or(raw_output.quality).unwrap_or(DEFAULT_QUALITY);
    // CRF range depends on which encoder the chosen container actually uses;
    // catching an out-of-range value here gives a proper diagnostic
    // instead of letting it fail deep inside FFmpeg as an opaque encoder error.
    let quality_limit = match format {
        OutputFormat::WebM => Some(MAX_QUALITY_VP9),
        OutputFormat::Mp4 => Some(MAX_QUALITY_H264),
        OutputFormat::Png | OutputFormat::Gif => None,
    };
    if let Some(limit) = quality_limit {
        if quality > limit {
            errors.push(
                Diagnostic::error(
                    codes::BAD_QUALITY,
                    format!(
                        "output.quality {quality} is out of range for {} (expected 0-{limit})",
                        format.as_str()
                    ),
                )
                .at_path("output.quality")
                .with_hint(format!(
                    "Use a value between 0 and {limit} (lower is higher quality)."
                )),
            );
        }
    }

    let output = OutputConfig {
        format,
        codec,
        path: output_path,
        size: Size { width, height },
        fps,
        transparent,
        quality,
        overwrite: ov.overwrite,
    };

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(Loaded {
        scenario: Scenario {
            metadata: Metadata {
                name,
                description: raw.metadata.and_then(|m| m.description),
            },
            canvas: CanvasConfig {
                size: Size { width, height },
                fps,
                background: BackgroundConfig::Solid(background_color),
                transparent,
            },
            terminal: TerminalConfig {
                bounds,
                padding,
                overflow,
            },
            font,
            prompt,
            cursor,
            palette,
            playback,
            seed: raw.random.and_then(|r| r.seed).unwrap_or(0),
            timeline,
        },
        output,
    })
}

fn duration_or_zero(text: &str, path: &str, errors: &mut Vec<Diagnostic>) -> Time {
    match parse_duration(text) {
        Ok(time) => time,
        Err(err) => {
            errors.push(Diagnostic::error(codes::BAD_DURATION, err.to_string()).at_path(path));
            Time::ZERO
        }
    }
}

fn positive_duration(text: &str, path: &str, errors: &mut Vec<Diagnostic>) -> Time {
    let time = duration_or_zero(text, path, errors);
    if time == Time::ZERO {
        errors.push(
            Diagnostic::error(
                codes::DURATION_NOT_POSITIVE,
                format!("{path} must be greater than 0"),
            )
            .at_path(path),
        );
    }
    time
}

fn resolve_spans(
    index: usize,
    text: &Option<String>,
    spans: &Option<Vec<RawSpan>>,
    palette: &Palette,
    errors: &mut Vec<Diagnostic>,
) -> Vec<TextRun> {
    if let Some(spans) = spans {
        return spans
            .iter()
            .enumerate()
            .map(|(i, span)| TextRun {
                text: span.text.clone(),
                color: span.color.as_deref().and_then(|name| {
                    match palette_color(name, palette) {
                        Some(color) => Some(color),
                        None => {
                            errors.push(
                                Diagnostic::error(
                                    codes::BAD_COLOR,
                                    format!("unknown color `{name}`"),
                                )
                                .at_path(format!("timeline[{index}].spans[{i}].color"))
                                .with_hint(
                                    "Use a palette name (foreground, warning, error, success, muted, command, cursor) or a hex value like `#39FF14`.",
                                ),
                            );
                            None
                        }
                    }
                }),
            })
            .collect();
    }

    vec![TextRun {
        text: text.clone().unwrap_or_default(),
        color: None,
    }]
}

/// Resolves a palette name, or a literal hex color.
fn palette_color(name: &str, palette: &Palette) -> Option<Color> {
    match name {
        "foreground" => Some(palette.foreground),
        "background" => Some(palette.background),
        "command" => Some(palette.command),
        "cursor" => Some(palette.cursor),
        "success" => Some(palette.success),
        "warning" => Some(palette.warning),
        "error" => Some(palette.error),
        "muted" => Some(palette.muted),
        other => parse_color(other).ok(),
    }
}

/// Default per-grapheme typing speed for `write` and `write_line` when no
/// `speed` is given. 35ms/char reads as fast, confident typing on screen.
const DEFAULT_WRITE_SPEED: Time = Time::from_millis(35);
/// Default per-grapheme typing speed for `command`. Slightly slower than
/// plain typing so a typed command reads as deliberate.
const DEFAULT_COMMAND_SPEED: Time = Time::from_millis(45);
/// Default per-character speed for `backspace`.
const DEFAULT_BACKSPACE_SPEED: Time = Time::from_millis(40);
/// Default pause between a command finishing and its Enter keypress landing.
const DEFAULT_ENTER_DELAY: Time = Time::from_millis(250);

fn resolve_action(
    index: usize,
    action: &RawAction,
    palette: &Palette,
    errors: &mut Vec<Diagnostic>,
) -> Action {
    let speed_of = |value: &Option<String>, errors: &mut Vec<Diagnostic>, default: Time| match value
    {
        None => default,
        Some(text) => duration_or_zero(text, &format!("timeline[{index}].speed"), errors),
    };

    match action {
        RawAction::Write { text, spans, speed } => Action::Write {
            spans: resolve_spans(index, text, spans, palette, errors),
            speed: speed_of(speed, errors, DEFAULT_WRITE_SPEED),
        },
        RawAction::WriteLine { text, spans, speed } => Action::WriteLine {
            spans: resolve_spans(index, text, spans, palette, errors),
            speed: speed_of(speed, errors, DEFAULT_WRITE_SPEED),
        },
        RawAction::Command {
            text,
            speed,
            enter_delay,
        } => Action::Command {
            text: text.clone(),
            speed: speed_of(speed, errors, DEFAULT_COMMAND_SPEED),
            enter_delay: match enter_delay {
                None => DEFAULT_ENTER_DELAY,
                Some(value) => {
                    duration_or_zero(value, &format!("timeline[{index}].enter_delay"), errors)
                }
            },
        },
        RawAction::Pause { duration } => Action::Pause {
            duration: positive_duration(duration, &format!("timeline[{index}].duration"), errors),
        },
        RawAction::PauseRandom { min, max, .. } => {
            let min_time = positive_duration(min, &format!("timeline[{index}].min"), errors);
            let max_time = positive_duration(max, &format!("timeline[{index}].max"), errors);
            if max_time < min_time {
                errors.push(
                    Diagnostic::error(
                        codes::DURATION_NOT_POSITIVE,
                        "pause_random max must be greater than or equal to min",
                    )
                    .at_path(format!("timeline[{index}].max")),
                );
            }
            Action::PauseRandom {
                min: min_time,
                max: max_time,
            }
        }
        RawAction::Newline { count } => {
            let count = count.unwrap_or(1);
            if count == 0 {
                errors.push(
                    Diagnostic::error(
                        codes::DURATION_NOT_POSITIVE,
                        "newline count must be at least 1",
                    )
                    .at_path(format!("timeline[{index}].count")),
                );
            }
            Action::Newline {
                count: count.max(1),
            }
        }
        RawAction::Clear { effect } => Action::Clear {
            effect: match effect.as_deref() {
                None | Some("instant") => ClearEffect::Instant,
                Some("terminal") => ClearEffect::Terminal,
                Some(other) => {
                    errors.push(
                        Diagnostic::error(
                            codes::UNKNOWN_FIELD,
                            format!(
                                "unknown clear effect `{other}` (expected instant or terminal)"
                            ),
                        )
                        .at_path(format!("timeline[{index}].effect")),
                    );
                    ClearEffect::Instant
                }
            },
        },
        RawAction::Backspace { count, speed } => {
            let count = count.unwrap_or(1);
            if count == 0 {
                errors.push(
                    Diagnostic::error(
                        codes::DURATION_NOT_POSITIVE,
                        "backspace count must be at least 1",
                    )
                    .at_path(format!("timeline[{index}].count")),
                );
            }
            Action::Backspace {
                count: count.max(1),
                speed: speed_of(speed, errors, DEFAULT_BACKSPACE_SPEED),
            }
        }
        RawAction::Cursor {
            visible,
            blink,
            duration,
            ..
        } => Action::Cursor {
            visible: visible.unwrap_or(true),
            blink: blink
                .as_deref()
                .map(|text| duration_or_zero(text, &format!("timeline[{index}].blink"), errors)),
            duration: duration
                .as_deref()
                .map(|text| positive_duration(text, &format!("timeline[{index}].duration"), errors))
                .unwrap_or(Time::ZERO),
        },
        RawAction::SetColor {
            foreground,
            background,
        } => {
            let convert = |value: &Option<String>, field: &str, errors: &mut Vec<Diagnostic>| {
                value
                    .as_deref()
                    .and_then(|name| match palette_color(name, palette) {
                        Some(color) => Some(color),
                        None => {
                            errors.push(
                                Diagnostic::error(
                                    codes::BAD_COLOR,
                                    format!("unknown color `{name}`"),
                                )
                                .at_path(format!("timeline[{index}].{field}")),
                            );
                            None
                        }
                    })
            };
            Action::SetColor {
                foreground: convert(foreground, "foreground", errors),
                background: convert(background, "background", errors),
            }
        }
        RawAction::ResetStyle => Action::ResetStyle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termotion_core::{Action, Fps, Time};

    fn resolve_str(src: &str) -> Result<Loaded, Vec<Diagnostic>> {
        let raw = crate::raw::parse(src).map_err(|d| vec![d])?;
        resolve(raw, ResolveContext::default())
    }

    #[test]
    fn applies_documented_defaults() {
        let loaded = resolve_str("version: 1\ntimeline: []\n").unwrap();
        assert_eq!(loaded.scenario.canvas.size.width, 1920);
        assert_eq!(loaded.scenario.canvas.fps, Fps::from_integer(30));
        assert_eq!(loaded.scenario.font.size, 40.0);
    }

    #[test]
    fn scenario_values_beat_defaults() {
        let loaded =
            resolve_str("version: 1\ncanvas:\n  width: 1280\n  height: 720\n  fps: 60\n").unwrap();
        assert_eq!(loaded.scenario.canvas.size.width, 1280);
        assert_eq!(loaded.scenario.canvas.fps, Fps::from_integer(60));
    }

    #[test]
    fn cli_overrides_beat_the_scenario() {
        let raw = crate::raw::parse("version: 1\ncanvas:\n  fps: 30\n").unwrap();
        let ctx = ResolveContext {
            overrides: Overrides {
                fps: Some(60),
                width: Some(3840),
                ..Overrides::default()
            },
            ..ResolveContext::default()
        };
        let loaded = resolve(raw, ctx).unwrap();
        assert_eq!(loaded.scenario.canvas.fps, Fps::from_integer(60));
        assert_eq!(loaded.scenario.canvas.size.width, 3840);
    }

    #[test]
    fn parses_durations_into_concrete_times() {
        let loaded =
            resolve_str("version: 1\ntimeline:\n  - type: pause\n    duration: 2.5s\n").unwrap();
        match loaded.scenario.timeline[0] {
            Action::Pause { duration } => assert_eq!(duration, Time::from_millis(2_500)),
            ref other => panic!("expected pause, got {other:?}"),
        }
    }

    #[test]
    fn a_plain_write_becomes_a_single_uncolored_run() {
        let loaded =
            resolve_str("version: 1\ntimeline:\n  - type: write\n    text: hello\n").unwrap();
        match &loaded.scenario.timeline[0] {
            Action::Write { spans, .. } => {
                assert_eq!(spans.len(), 1);
                assert_eq!(spans[0].text, "hello");
                assert!(spans[0].color.is_none());
            }
            other => panic!("expected write, got {other:?}"),
        }
    }

    #[test]
    fn span_color_names_resolve_against_the_palette() {
        let src = "version: 1\ntheme:\n  ref: zombocoder\ntimeline:\n  - type: write\n    spans:\n      - text: hot\n        color: warning\n";
        let loaded = resolve_str(src).unwrap();
        match &loaded.scenario.timeline[0] {
            Action::Write { spans, .. } => {
                assert_eq!(spans[0].color, Some(loaded.scenario.palette.warning));
            }
            other => panic!("expected write, got {other:?}"),
        }
    }

    #[test]
    fn zero_pause_duration_is_error_e014() {
        let errs =
            resolve_str("version: 1\ntimeline:\n  - type: pause\n    duration: 0ms\n").unwrap_err();
        assert_eq!(errs[0].code, codes::DURATION_NOT_POSITIVE);
        assert_eq!(errs[0].path.as_deref(), Some("timeline[0].duration"));
    }

    #[test]
    fn zero_canvas_dimensions_are_rejected() {
        let errs = resolve_str("version: 1\ncanvas:\n  width: 0\n").unwrap_err();
        assert_eq!(errs[0].code, codes::BAD_DIMENSION);
    }

    #[test]
    fn an_absurd_canvas_is_rejected_instead_of_reaching_render() {
        // Regression test: `validate` must reject this the same way `render`
        // would otherwise fail on it (an OOM kill with no diagnostic).
        let errs = resolve_str("version: 1\ncanvas:\n  width: 4294967295\n  height: 4294967295\n")
            .unwrap_err();
        assert!(
            errs.iter().any(|e| e.code == codes::BAD_DIMENSION),
            "expected BAD_DIMENSION among {errs:?}"
        );
    }

    #[test]
    fn a_canvas_dimension_just_over_the_limit_is_rejected() {
        let src = format!(
            "version: 1\ncanvas:\n  width: {}\n  height: 1080\n",
            termotion_core::MAX_CANVAS_DIMENSION + 1
        );
        let errs = resolve_str(&src).unwrap_err();
        assert_eq!(errs[0].code, codes::BAD_DIMENSION);
    }

    #[test]
    fn a_canvas_at_exactly_the_limit_is_accepted() {
        let src = format!(
            "version: 1\ncanvas:\n  width: {}\n  height: {}\n  fps: 24\noutput:\n  format: webm\n",
            termotion_core::MAX_CANVAS_DIMENSION,
            termotion_core::MAX_CANVAS_DIMENSION
        );
        assert!(resolve_str(&src).is_ok());
    }

    #[test]
    fn an_output_path_starting_with_a_dash_is_rejected() {
        let errs = resolve_str("version: 1\noutput:\n  path: \"-loglevel\"\n").unwrap_err();
        assert_eq!(errs[0].code, codes::BAD_OUTPUT_PATH);
        assert_eq!(errs[0].path.as_deref(), Some("output.path"));
    }

    #[test]
    fn an_output_path_starting_with_a_dash_is_rejected_even_with_a_directory_prefix() {
        let errs =
            resolve_str("version: 1\noutput:\n  path: \"out/-loglevel.webm\"\n").unwrap_err();
        assert_eq!(errs[0].code, codes::BAD_OUTPUT_PATH);
    }

    #[test]
    fn an_ordinary_output_path_is_accepted() {
        let loaded = resolve_str("version: 1\noutput:\n  path: \"out/render.webm\"\n").unwrap();
        assert_eq!(loaded.output.path, PathBuf::from("out/render.webm"));
    }

    #[test]
    fn zero_fps_is_rejected() {
        let errs = resolve_str("version: 1\ncanvas:\n  fps: 0\n").unwrap_err();
        assert_eq!(errs[0].code, codes::BAD_DIMENSION);
    }

    #[test]
    fn a_terminal_outside_the_canvas_is_rejected() {
        let errs = resolve_str("version: 1\ncanvas:\n  width: 640\n  height: 480\nterminal:\n  x: 100\n  y: 100\n  width: 2000\n  height: 200\n").unwrap_err();
        assert_eq!(errs[0].code, codes::TERMINAL_OUT_OF_BOUNDS);
    }

    #[test]
    fn odd_dimensions_are_rejected_only_for_mp4() {
        let src = "version: 1\ncanvas:\n  width: 1921\n  height: 1080\noutput:\n  format: mp4\n";
        let errs = resolve_str(src).unwrap_err();
        assert_eq!(errs[0].code, codes::ODD_DIMENSION_FOR_H264);

        let webm = "version: 1\ncanvas:\n  width: 1921\n  height: 1080\noutput:\n  format: webm\n";
        assert!(resolve_str(webm).is_ok());
    }

    #[test]
    fn quality_above_the_codecs_crf_range_is_rejected() {
        // VP9's `-crf` tops out at 63; a value above that would otherwise sail
        // through to FFmpeg and fail deep inside the encoder instead of here.
        let webm = "version: 1\noutput:\n  format: webm\n  quality: 90\n";
        let errs = resolve_str(webm).unwrap_err();
        assert_eq!(errs[0].code, codes::BAD_QUALITY);
        assert_eq!(errs[0].path.as_deref(), Some("output.quality"));

        // H.264's `-crf` tops out at 51, so 55 is invalid for mp4 even though
        // it would be in range for webm/vp9.
        let mp4 = "version: 1\ncanvas:\n  width: 640\n  height: 480\noutput:\n  format: mp4\n  quality: 55\n";
        let errs = resolve_str(mp4).unwrap_err();
        assert_eq!(errs[0].code, codes::BAD_QUALITY);
    }

    #[test]
    fn quality_within_range_is_accepted_for_both_codecs() {
        let webm = "version: 1\noutput:\n  format: webm\n  quality: 63\n";
        assert!(resolve_str(webm).is_ok());

        let mp4 = "version: 1\ncanvas:\n  width: 640\n  height: 480\noutput:\n  format: mp4\n  quality: 51\n";
        assert!(resolve_str(mp4).is_ok());
    }

    #[test]
    fn zero_newline_and_backspace_counts_are_rejected() {
        let errs =
            resolve_str("version: 1\ntimeline:\n  - type: newline\n    count: 0\n").unwrap_err();
        assert_eq!(errs[0].code, codes::DURATION_NOT_POSITIVE);
    }

    #[test]
    fn pause_random_requires_min_below_max() {
        let errs = resolve_str(
            "version: 1\ntimeline:\n  - type: pause_random\n    min: 600ms\n    max: 300ms\n",
        )
        .unwrap_err();
        assert_eq!(errs[0].code, codes::DURATION_NOT_POSITIVE);
    }

    #[test]
    fn all_validation_errors_are_collected_not_just_the_first() {
        let src = "version: 1\ncanvas:\n  width: 0\n  fps: 0\ntimeline:\n  - type: pause\n    duration: 0ms\n";
        let errs = resolve_str(src).unwrap_err();
        assert!(
            errs.len() >= 3,
            "expected several diagnostics, got {errs:?}"
        );
    }

    #[test]
    fn bad_colors_report_their_field_path() {
        let errs = resolve_str("version: 1\ncanvas:\n  background: 'zzz'\n").unwrap_err();
        assert_eq!(errs[0].code, codes::BAD_COLOR);
        assert_eq!(errs[0].path.as_deref(), Some("canvas.background"));
    }

    #[test]
    fn transparent_canvas_propagates_to_the_output_config() {
        let loaded = resolve_str("version: 1\ncanvas:\n  transparent: true\n").unwrap();
        assert!(loaded.scenario.canvas.transparent);
        assert!(loaded.output.transparent);
    }

    #[test]
    fn metadata_name_defaults_to_untitled() {
        let loaded = resolve_str("version: 1\n").unwrap();
        assert_eq!(loaded.scenario.metadata.name, "untitled");
    }
}
