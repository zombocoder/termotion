use std::path::Path;

use termotion_render::{list_families, FontSource, FontStack};
use termotion_schema::diag::{codes, Diagnostic};
use termotion_schema::resolve::Overrides;

use crate::report;

/// Show available font families, or — with `--scenario` — what a specific
/// scenario's `font:` block resolves to (family, origin, size, and derived
/// metrics), which is the diagnostic a user needs when their text looks wrong.
pub fn run(scenario: Option<&Path>) -> i32 {
    match scenario {
        Some(path) => report_scenario_font(path),
        None => {
            println!("available families:");
            for family in list_families() {
                println!("  {family}");
            }
            0
        }
    }
}

fn report_scenario_font(path: &Path) -> i32 {
    let loaded = match termotion_schema::load(path, &Overrides::default()) {
        Ok(loaded) => loaded,
        Err(errors) => {
            report::print_diagnostics(&errors);
            return report::exit_code_for(&errors);
        }
    };

    match FontStack::load(&loaded.scenario.font) {
        Ok(stack) => {
            let metrics = stack.metrics();
            let origin = match stack.source() {
                FontSource::Embedded => "embedded".to_string(),
                FontSource::System(name) => format!("system ({name})"),
                FontSource::File(path) => format!("file ({})", path.display()),
            };
            println!("resolved: {} [{origin}]", stack.family());
            println!("size: {}", stack.size());
            println!("advance: {}px", metrics.advance_w);
            println!("line height: {}px", metrics.line_h);
            0
        }
        Err(err) => {
            let diagnostic = Diagnostic::error(codes::FONT_NOT_FOUND, err.to_string())
                .at_path("font.family")
                .in_file(path)
                .with_hint(
                    "Install the font or specify:\n\nfont:\n  path: ./assets/fonts/font.ttf",
                );
            let diagnostics = std::slice::from_ref(&diagnostic);
            report::print_diagnostics(diagnostics);
            report::exit_code_for(diagnostics)
        }
    }
}
