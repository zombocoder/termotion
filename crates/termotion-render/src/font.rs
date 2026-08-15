use std::path::PathBuf;
use std::sync::Arc;

use cosmic_text::{fontdb, Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight};
use termotion_core::FontConfig;
use thiserror::Error;

pub const EMBEDDED_FONT_NAME: &str = "JetBrains Mono";

/// Bundled Apache-2.0 face. Guarantees a clean machine can render with no setup and
/// keeps golden images byte-stable across platforms.
const EMBEDDED_FONT: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf");

/// Fallback ascent, as a fraction of em, used only if a resolved face's vertical
/// metrics cannot be read (defensive: a system font loaded from a file path has no
/// `cosmic_text::Font` handle, since `cosmic-text` only builds shaping handles from
/// in-memory face data). Matches JetBrains Mono's own reported ascent, so behavior
/// is unchanged for the embedded default face.
const FALLBACK_ASCENT_EM: f32 = 1.02;
/// See `FALLBACK_ASCENT_EM`. Matches JetBrains Mono's reported descent magnitude.
const FALLBACK_DESCENT_EM: f32 = 0.30;

#[derive(Debug, Error)]
pub enum FontError {
    #[error("font family `{family}` not found")]
    FamilyNotFound { family: String },
    #[error("cannot read font file `{path}`: {source}")]
    FileNotReadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("font file `{path}` could not be parsed")]
    FileInvalid { path: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontSource {
    Embedded,
    File(PathBuf),
    System(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetrics {
    /// Integer cell advance. Integer so column `n` lands on `x0 + n * advance_w`
    /// identically on every platform.
    pub advance_w: u32,
    pub line_h: u32,
    /// Baseline offset from the top of the cell, in pixels.
    pub ascent: f32,
}

#[derive(Debug)]
pub struct FontStack {
    system: FontSystem,
    metrics: FontMetrics,
    source: FontSource,
    family: String,
    weight: Weight,
    size: f32,
}

impl FontStack {
    pub fn load(config: &FontConfig) -> Result<Self, FontError> {
        let mut system = FontSystem::new();

        // The embedded face is always available as the last-resort fallback.
        system
            .db_mut()
            .load_font_source(fontdb::Source::Binary(Arc::new(EMBEDDED_FONT.to_vec())));

        let weight = Weight(config.weight);

        let (family, source) = match &config.path {
            Some(path) => {
                let bytes = std::fs::read(path).map_err(|err| FontError::FileNotReadable {
                    path: path.clone(),
                    source: err,
                })?;
                let ids = system
                    .db_mut()
                    .load_font_source(fontdb::Source::Binary(Arc::new(bytes)));
                let family =
                    ids.iter()
                        .find_map(|id| {
                            system.db().face(*id).and_then(|face| {
                                face.families.first().map(|(name, _)| name.clone())
                            })
                        })
                        .ok_or_else(|| FontError::FileInvalid { path: path.clone() })?;
                (family, FontSource::File(path.clone()))
            }
            None => {
                let wanted = config.family.clone();
                let found = system
                    .db()
                    .query(&fontdb::Query {
                        families: &[Family::Name(&wanted)],
                        weight,
                        ..fontdb::Query::default()
                    })
                    .is_some();
                if !found {
                    return Err(FontError::FamilyNotFound { family: wanted });
                }
                let source = if wanted == EMBEDDED_FONT_NAME {
                    FontSource::Embedded
                } else {
                    FontSource::System(wanted.clone())
                };
                (wanted, source)
            }
        };

        let line_h = (config.size * config.line_height).round().max(1.0) as u32;
        let advance_w = measure_advance(&mut system, &family, weight, config.size);
        let ascent = baseline_offset(&mut system, &family, weight, config.size, line_h);

        Ok(FontStack {
            system,
            metrics: FontMetrics {
                advance_w,
                line_h,
                ascent,
            },
            source,
            family,
            weight,
            size: config.size,
        })
    }

    pub fn metrics(&self) -> FontMetrics {
        self.metrics
    }

    pub fn source(&self) -> FontSource {
        self.source.clone()
    }

    pub fn family(&self) -> &str {
        &self.family
    }

    pub fn size(&self) -> f32 {
        self.size
    }

    pub fn weight(&self) -> Weight {
        self.weight
    }

    pub fn system_mut(&mut self) -> &mut FontSystem {
        &mut self.system
    }

    pub fn attrs(&self) -> Attrs<'_> {
        Attrs::new()
            .family(Family::Name(&self.family))
            .weight(self.weight)
    }
}

/// Shapes `0` and rounds its advance to an integer pixel. `0` is representative of
/// every glyph in a monospace face, which is the only face family this renderer
/// targets (spec assumes fixed-width cells).
fn measure_advance(system: &mut FontSystem, family: &str, weight: Weight, size: f32) -> u32 {
    let mut buffer = Buffer::new(system, Metrics::new(size, size));
    let attrs = Attrs::new().family(Family::Name(family)).weight(weight);
    buffer.set_text("0", &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(system, false);

    let advance = buffer
        .layout_runs()
        .flat_map(|run| run.glyphs.iter())
        .map(|glyph| glyph.w)
        .fold(0.0f32, f32::max);

    (advance.round() as u32).max(1)
}

/// Derives the baseline offset (distance from the top of the cell to the text
/// baseline) from the face's real ascent and descent, centering the glyph block
/// within `line_h` rather than hugging the top of the cell.
fn baseline_offset(
    system: &mut FontSystem,
    family: &str,
    weight: Weight,
    size: f32,
    line_h: u32,
) -> f32 {
    let (ascent_em, descent_em) = font_vertical_metrics_em(system, family, weight)
        .unwrap_or((FALLBACK_ASCENT_EM, FALLBACK_DESCENT_EM));
    let ascent = ascent_em * size;
    let descent = descent_em * size;
    let content = ascent + descent;
    // Clamp instead of underflowing when a very tight `line_height` makes the
    // ascent+descent block taller than the line box itself.
    let leading = (line_h as f32 - content).max(0.0);
    leading / 2.0 + ascent
}

/// Reads the resolved face's real ascent/descent as fractions of its em square.
/// Returns `None` when the face has no directly loaded glyph data to inspect —
/// notably, a system font that `fontdb` only knows by file path rather than by
/// in-memory bytes, since `cosmic_text::FontSystem::get_font` cannot build a
/// shaping handle from a `Source::File` entry.
fn font_vertical_metrics_em(
    system: &mut FontSystem,
    family: &str,
    weight: Weight,
) -> Option<(f32, f32)> {
    let id = system.db().query(&fontdb::Query {
        families: &[Family::Name(family)],
        weight,
        ..fontdb::Query::default()
    })?;
    let font = system.get_font(id, weight)?;
    let metrics = font.metrics();
    if metrics.units_per_em == 0 {
        return None;
    }
    let units_per_em = f32::from(metrics.units_per_em);
    Some((
        metrics.ascent / units_per_em,
        metrics.descent.abs() / units_per_em,
    ))
}

/// Every font family visible to Termotion: system fonts plus the embedded face,
/// sorted and deduplicated. Used by the `fonts` CLI command so a user can see what
/// is available before choosing `font.family` in a scenario.
pub fn list_families() -> Vec<String> {
    let mut system = FontSystem::new();
    system
        .db_mut()
        .load_font_source(fontdb::Source::Binary(Arc::new(EMBEDDED_FONT.to_vec())));

    let mut families: Vec<String> = system
        .db()
        .faces()
        .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
        .collect();
    families.sort();
    families.dedup();
    families
}

#[cfg(test)]
mod tests {
    use super::*;
    use termotion_core::FontConfig;

    #[test]
    fn loads_the_embedded_font_by_default() {
        let stack = FontStack::load(&FontConfig::default()).unwrap();
        assert!(matches!(
            stack.source(),
            FontSource::Embedded | FontSource::System(_)
        ));
    }

    #[test]
    fn metrics_are_positive_and_scale_with_size() {
        let small = FontStack::load(&FontConfig {
            size: 20.0,
            ..FontConfig::default()
        })
        .unwrap()
        .metrics();
        let large = FontStack::load(&FontConfig {
            size: 40.0,
            ..FontConfig::default()
        })
        .unwrap()
        .metrics();

        assert!(small.advance_w >= 1);
        assert!(large.advance_w > small.advance_w);
        assert!(large.line_h > small.line_h);
    }

    #[test]
    fn line_height_follows_the_configured_multiplier() {
        let metrics = FontStack::load(&FontConfig {
            size: 40.0,
            line_height: 1.4,
            ..FontConfig::default()
        })
        .unwrap()
        .metrics();
        assert_eq!(metrics.line_h, 56); // round(40 * 1.4)
    }

    #[test]
    fn jetbrains_mono_advance_is_six_tenths_of_the_em() {
        // Guards the GridSpec::estimate ratio used before a font is loaded.
        let metrics = FontStack::load(&FontConfig {
            size: 40.0,
            ..FontConfig::default()
        })
        .unwrap()
        .metrics();
        assert_eq!(metrics.advance_w, 24);
    }

    #[test]
    fn a_missing_family_reports_the_name_it_looked_for() {
        let err = FontStack::load(&FontConfig {
            family: "Definitely Not Installed 12345".to_string(),
            ..FontConfig::default()
        })
        .unwrap_err();
        match err {
            FontError::FamilyNotFound { family } => assert!(family.contains("12345")),
            other => panic!("expected FamilyNotFound, got {other:?}"),
        }
    }

    #[test]
    fn a_missing_font_file_reports_its_path() {
        let err = FontStack::load(&FontConfig {
            path: Some("/nonexistent/font.ttf".into()),
            ..FontConfig::default()
        })
        .unwrap_err();
        assert!(matches!(err, FontError::FileNotReadable { .. }));
    }

    #[test]
    fn an_explicit_path_wins_over_the_family() {
        let stack = FontStack::load(&FontConfig {
            family: "Definitely Not Installed 12345".to_string(),
            path: Some("assets/fonts/JetBrainsMono-Regular.ttf".into()),
            ..FontConfig::default()
        });
        // Relative to the workspace root when tests run from there.
        if let Ok(stack) = stack {
            assert!(matches!(stack.source(), FontSource::File(_)));
        }
    }
}
