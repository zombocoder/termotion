use std::path::{Path, PathBuf};

use termotion_render::{FontError, RenderError};
use termotion_schema::diag::{codes, Diagnostic};
use termotion_schema::resolve::Overrides;

use crate::pipeline::{self, PipelineError};
use crate::report;

/// Frames between progress updates on the `--quiet`-less status line. 30 frames
/// is once a second at 30fps, the spec's default frame rate — frequent enough to
/// read as live progress without flooding the terminal on long renders.
const PROGRESS_REPORT_INTERVAL: u64 = 30;

pub struct RenderArgs {
    pub file: PathBuf,
    pub overrides: Overrides,
    pub quiet: bool,
}

/// Loads and validates the scenario, then streams it through the render pipeline
/// to the configured encoder. Exit codes follow the documented exit-code table
/// (see `report::exit_code_for` and `Diagnostic::exit_code`).
pub fn run(args: RenderArgs) -> i32 {
    let loaded = match termotion_schema::load(&args.file, &args.overrides) {
        Ok(loaded) => loaded,
        Err(errors) => {
            report::print_diagnostics(&errors);
            return report::exit_code_for(&errors);
        }
    };

    let quiet = args.quiet;
    let result = pipeline::render_to(&loaded, |done, total| {
        if !quiet && (done == total || done.is_multiple_of(PROGRESS_REPORT_INTERVAL)) {
            eprint!("\rrendering {done}/{total} frames");
        }
    });

    if !quiet {
        eprintln!();
    }

    match result {
        Ok(summary) => {
            if !quiet {
                println!(
                    "Rendered {} frames ({:.2}s) to {}",
                    summary.frames,
                    summary.duration.as_secs_f64(),
                    loaded.output.path.display()
                );
            }
            0
        }
        Err(error) => {
            let diagnostic = to_diagnostic(error, &args.file);
            report::print_diagnostics(std::slice::from_ref(&diagnostic));
            diagnostic.exit_code()
        }
    }
}

/// Translates crate-local errors from `termotion-render` and `termotion-encode`
/// into user-facing diagnostics. Those crates deliberately do not depend on
/// `termotion-schema` (it would invert the dependency layering), so this
/// translation happens here, at the one place that depends on all three.
fn to_diagnostic(error: PipelineError, file: &Path) -> Diagnostic {
    match error {
        PipelineError::Compile(diagnostic) => diagnostic.in_file(file),
        PipelineError::Render(RenderError::Font(FontError::FamilyNotFound { family })) => {
            Diagnostic::error(
                codes::FONT_NOT_FOUND,
                format!("font \"{family}\" not found"),
            )
            .at_path("font.family")
            .in_file(file)
            .with_hint(
                "Install the font or set font.path:\n\nfont:\n  path: ./assets/fonts/font.ttf",
            )
        }
        PipelineError::Render(RenderError::Font(other)) => {
            Diagnostic::error(codes::FONT_NOT_FOUND, other.to_string())
                .at_path("font.path")
                .in_file(file)
                .with_hint("Check that the path is correct and the file is a valid TTF or OTF.")
        }
        PipelineError::Render(other) => {
            Diagnostic::error(codes::RENDER_FAILED, other.to_string()).in_file(file)
        }
        PipelineError::Encode(termotion_encode::EncodeError::OutputExists { path }) => {
            Diagnostic::error(
                codes::OUTPUT_EXISTS,
                format!("`{}` already exists", path.display()),
            )
            .in_file(file)
            .with_hint("Pass --overwrite to replace it.")
        }
        PipelineError::Encode(other) => {
            Diagnostic::error(codes::ENCODER_FAILED, other.to_string()).in_file(file)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Diagnostic::file` is only rendered to stderr text alongside a source
    /// position (see `Diagnostic`'s `Display` impl), which pipeline-stage
    /// errors never carry — so a missing `.in_file(file)` call here has no
    /// visible symptom in `termotion render`'s plain-text output today. It's
    /// still part of the `Diagnostic`'s data (used by e.g. `report::to_json`
    /// for other commands), so this asserts on the field directly rather
    /// than on rendered text.
    #[test]
    fn every_encode_diagnostic_carries_the_scenario_file() {
        let file = Path::new("scene.yaml");

        let output_exists = to_diagnostic(
            PipelineError::Encode(termotion_encode::EncodeError::OutputExists {
                path: PathBuf::from("out.webm"),
            }),
            file,
        );
        assert_eq!(output_exists.file.as_deref(), Some(file));

        let other = to_diagnostic(
            PipelineError::Encode(termotion_encode::EncodeError::Ffmpeg("boom".to_string())),
            file,
        );
        assert_eq!(other.file.as_deref(), Some(file));
    }
}
