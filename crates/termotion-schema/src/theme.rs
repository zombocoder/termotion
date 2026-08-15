use std::path::PathBuf;

use serde::Deserialize;
use termotion_core::{parse_color, Color, Palette};

use crate::diag::{codes, Diagnostic};

/// Theme files accept both the flat form (`colors:` map) and the nested
/// form (`prompt:` / `status:` sub-maps). Anything unset falls back to defaults.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTheme {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    colors: Option<RawColors>,
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    foreground: Option<String>,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    prompt: Option<RawPromptColors>,
    #[serde(default)]
    status: Option<RawStatusColors>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawColors {
    background: Option<String>,
    foreground: Option<String>,
    prompt_user: Option<String>,
    prompt_host: Option<String>,
    prompt_path: Option<String>,
    prompt_symbol: Option<String>,
    command: Option<String>,
    cursor: Option<String>,
    success: Option<String>,
    warning: Option<String>,
    error: Option<String>,
    muted: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPromptColors {
    user: Option<String>,
    host: Option<String>,
    path: Option<String>,
    symbol: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStatusColors {
    success: Option<String>,
    pending: Option<String>,
    error: Option<String>,
}

const fn palette(background: Color, foreground: Color, accent: Color, muted: Color) -> Palette {
    Palette {
        background,
        foreground,
        prompt_user: accent,
        prompt_host: accent,
        prompt_path: muted,
        prompt_symbol: accent,
        command: foreground,
        cursor: accent,
        success: accent,
        warning: Color::rgb(0xFF, 0xD1, 0x66),
        error: Color::rgb(0xFF, 0x4D, 0x4D),
        muted,
    }
}

/// Order is stable: `themes list` prints them exactly like this.
pub static BUILTIN_THEMES: &[(&str, Palette)] = &[
    (
        "terminal-green",
        palette(
            Color::rgb(0x05, 0x07, 0x06),
            Color::rgb(0xB6, 0xF7, 0xB0),
            Color::rgb(0x39, 0xFF, 0x14),
            Color::rgb(0x66, 0x72, 0x66),
        ),
    ),
    (
        "terminal-amber",
        palette(
            Color::rgb(0x0A, 0x07, 0x02),
            Color::rgb(0xFF, 0xCC, 0x66),
            Color::rgb(0xFF, 0xB0, 0x00),
            Color::rgb(0x7A, 0x60, 0x30),
        ),
    ),
    (
        "terminal-white",
        palette(
            Color::rgb(0x0B, 0x0B, 0x0B),
            Color::rgb(0xE8, 0xE8, 0xE8),
            Color::rgb(0xFF, 0xFF, 0xFF),
            Color::rgb(0x8A, 0x8A, 0x8A),
        ),
    ),
    (
        "matrix-green",
        palette(
            Color::rgb(0x00, 0x05, 0x00),
            Color::rgb(0x66, 0xFF, 0x66),
            Color::rgb(0x00, 0xFF, 0x41),
            Color::rgb(0x2A, 0x6B, 0x2A),
        ),
    ),
    (
        "unix-dark",
        palette(
            Color::rgb(0x10, 0x14, 0x18),
            Color::rgb(0xC5, 0xCD, 0xD5),
            Color::rgb(0x7A, 0xA6, 0xDA),
            Color::rgb(0x5A, 0x64, 0x70),
        ),
    ),
    (
        "retro-crt",
        palette(
            Color::rgb(0x02, 0x0A, 0x04),
            Color::rgb(0x9C, 0xE8, 0x9C),
            Color::rgb(0x33, 0xDD, 0x55),
            Color::rgb(0x3E, 0x6B, 0x45),
        ),
    ),
    (
        "zombocoder",
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
        },
    ),
];

pub fn builtin(name: &str) -> Option<Palette> {
    BUILTIN_THEMES
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, palette)| *palette)
}

pub fn list_builtin_names() -> Vec<&'static str> {
    BUILTIN_THEMES.iter().map(|(name, _)| *name).collect()
}

/// Project themes shadow built-ins of the same name.
pub fn load_theme(name: &str, search_dirs: &[PathBuf]) -> Result<Palette, Diagnostic> {
    for dir in search_dirs {
        let candidate = dir.join(format!("{name}.yaml"));
        if candidate.is_file() {
            let source = std::fs::read_to_string(&candidate).map_err(|err| {
                Diagnostic::error(
                    codes::UNKNOWN_THEME,
                    format!("cannot read theme {}: {err}", candidate.display()),
                )
            })?;
            return palette_from_yaml(&source).map_err(|diag| diag.in_file(&candidate));
        }
    }

    builtin(name).ok_or_else(|| {
        Diagnostic::error(codes::UNKNOWN_THEME, format!("unknown theme `{name}`")).with_hint(
            format!(
                "Available built-in themes:\n  {}",
                list_builtin_names().join("\n  ")
            ),
        )
    })
}

pub fn palette_from_yaml(source: &str) -> Result<Palette, Diagnostic> {
    let raw: RawTheme = serde_yaml_ng::from_str(source)
        .map_err(|err| Diagnostic::error(codes::YAML_SYNTAX, format!("invalid theme: {err}")))?;
    let _ = raw.name;

    let mut palette = Palette::default();
    let colors = raw.colors.unwrap_or_default();

    let set = |slot: &mut Color, value: &Option<String>, field: &str| -> Result<(), Diagnostic> {
        if let Some(text) = value {
            *slot = parse_color(text).map_err(|err| {
                Diagnostic::error(codes::BAD_COLOR, format!("{field}: {err}")).at_path(field)
            })?;
        }
        Ok(())
    };

    set(
        &mut palette.background,
        &colors.background,
        "colors.background",
    )?;
    set(
        &mut palette.foreground,
        &colors.foreground,
        "colors.foreground",
    )?;
    set(
        &mut palette.prompt_user,
        &colors.prompt_user,
        "colors.prompt_user",
    )?;
    set(
        &mut palette.prompt_host,
        &colors.prompt_host,
        "colors.prompt_host",
    )?;
    set(
        &mut palette.prompt_path,
        &colors.prompt_path,
        "colors.prompt_path",
    )?;
    set(
        &mut palette.prompt_symbol,
        &colors.prompt_symbol,
        "colors.prompt_symbol",
    )?;
    set(&mut palette.command, &colors.command, "colors.command")?;
    set(&mut palette.cursor, &colors.cursor, "colors.cursor")?;
    set(&mut palette.success, &colors.success, "colors.success")?;
    set(&mut palette.warning, &colors.warning, "colors.warning")?;
    set(&mut palette.error, &colors.error, "colors.error")?;
    set(&mut palette.muted, &colors.muted, "colors.muted")?;

    // Nested form, applied after the flat form so it wins when both appear.
    set(&mut palette.background, &raw.background, "background")?;
    set(&mut palette.foreground, &raw.foreground, "foreground")?;
    set(&mut palette.cursor, &raw.cursor, "cursor")?;

    if let Some(prompt) = raw.prompt {
        set(&mut palette.prompt_user, &prompt.user, "prompt.user")?;
        set(&mut palette.prompt_host, &prompt.host, "prompt.host")?;
        set(&mut palette.prompt_path, &prompt.path, "prompt.path")?;
        set(&mut palette.prompt_symbol, &prompt.symbol, "prompt.symbol")?;
    }
    if let Some(status) = raw.status {
        set(&mut palette.success, &status.success, "status.success")?;
        set(&mut palette.warning, &status.pending, "status.pending")?;
        set(&mut palette.error, &status.error, "status.error")?;
    }

    Ok(palette)
}

#[cfg(test)]
mod tests {
    use super::*;
    use termotion_core::Color;

    #[test]
    fn every_documented_builtin_exists() {
        for name in [
            "terminal-green",
            "terminal-amber",
            "terminal-white",
            "matrix-green",
            "unix-dark",
            "retro-crt",
            "zombocoder",
        ] {
            assert!(builtin(name).is_some(), "missing built-in theme {name}");
        }
        assert!(builtin("nonexistent").is_none());
    }

    #[test]
    fn builtin_names_are_listed_in_a_stable_order() {
        let names = list_builtin_names();
        assert_eq!(names.first(), Some(&"terminal-green"));
        assert!(names.contains(&"zombocoder"));
    }

    #[test]
    fn parses_the_flat_color_map_form() {
        let src = "\
name: custom
colors:
  background: '#080b09'
  foreground: '#B6F7B0'
  prompt_user: '#39FF14'
  cursor: '#39FF14'
";
        let palette = palette_from_yaml(src).unwrap();
        assert_eq!(palette.background, Color::rgb(8, 11, 9));
        assert_eq!(palette.foreground, Color::rgb(0xB6, 0xF7, 0xB0));
        assert_eq!(palette.prompt_user, Color::rgb(0x39, 0xFF, 0x14));
    }

    #[test]
    fn parses_the_nested_prompt_and_status_form() {
        let src = "\
name: zombocoder
background: '#080B09'
foreground: '#C8E6C9'
prompt:
  user: '#39FF14'
  host: '#69D2FF'
  path: '#A7B0A7'
  symbol: '#39FF14'
status:
  success: '#39FF14'
  pending: '#F1C40F'
  error: '#FF4D4D'
cursor: '#39FF14'
";
        let palette = palette_from_yaml(src).unwrap();
        assert_eq!(palette.prompt_host, Color::rgb(0x69, 0xD2, 0xFF));
        assert_eq!(palette.warning, Color::rgb(0xF1, 0xC4, 0x0F));
        assert_eq!(palette.error, Color::rgb(0xFF, 0x4D, 0x4D));
    }

    #[test]
    fn unspecified_colors_fall_back_to_defaults() {
        let palette =
            palette_from_yaml("name: sparse\ncolors:\n  foreground: '#FFFFFF'\n").unwrap();
        assert_eq!(palette.foreground, Color::WHITE);
        assert_eq!(palette.muted, Palette::default().muted);
    }

    #[test]
    fn malformed_colors_report_a_color_error() {
        let err =
            palette_from_yaml("name: bad\ncolors:\n  foreground: 'not-a-color'\n").unwrap_err();
        assert_eq!(err.code, codes::BAD_COLOR);
        assert!(err.message.contains("foreground"));
    }

    #[test]
    fn unknown_theme_names_report_the_available_ones() {
        let err = load_theme("nope", &[]).unwrap_err();
        assert_eq!(err.code, codes::UNKNOWN_THEME);
        assert!(err.message.contains("nope"));
        assert!(err
            .hint
            .as_deref()
            .unwrap_or_default()
            .contains("terminal-green"));
    }
}
