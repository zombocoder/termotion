use std::path::Path;

use termotion_render::{grid_for, FontStack};
use termotion_schema::diag::{codes, Diagnostic};
use termotion_schema::resolve::Overrides;
use termotion_timeline::compile;

use crate::report;

/// Width of the left-aligned `{ms}ms` timestamp column in the `inspect` listing,
/// before the single separating space and the op description.
const TIMESTAMP_COLUMN_WIDTH: usize = 8;

pub fn run(path: &Path) -> i32 {
    let loaded = match termotion_schema::load(path, &Overrides::default()) {
        Ok(loaded) => loaded,
        Err(errors) => {
            report::print_diagnostics(&errors);
            return report::exit_code_for(&errors);
        }
    };

    // Loaded first, and the grid derived from its real shaped metrics via
    // `grid_for` — the same path `render` takes — so `inspect` prints the
    // grid that will actually render rather than `GridSpec::estimate`'s
    // fixed-ratio guess, which only agrees with real metrics for the
    // embedded default font.
    let font = match FontStack::load(&loaded.scenario.font) {
        Ok(font) => font,
        Err(err) => {
            let diagnostic = Diagnostic::error(codes::FONT_NOT_FOUND, err.to_string())
                .at_path("font.family")
                .in_file(path)
                .with_hint(
                    "Install the font or specify:\n\nfont:\n  path: ./assets/fonts/font.ttf",
                );
            report::print_diagnostics(std::slice::from_ref(&diagnostic));
            return diagnostic.exit_code();
        }
    };
    let grid = grid_for(&font, &loaded.scenario.terminal);
    drop(font);

    let program = match compile(&loaded.scenario, grid) {
        Ok(program) => program,
        Err(diagnostic) => {
            let diagnostic = diagnostic.in_file(path);
            report::print_diagnostics(std::slice::from_ref(&diagnostic));
            return diagnostic.exit_code();
        }
    };

    for event in &program.events {
        println!(
            "{:<TIMESTAMP_COLUMN_WIDTH$} {}",
            format!("{}ms", event.at.as_millis()),
            event.op.describe()
        );
    }

    println!();
    println!("Grid: {}x{} cells", grid.cols, grid.rows);
    println!("Events: {}", program.events.len());
    println!("Duration: {:.2}s", program.duration.as_secs_f64());
    println!(
        "Frames: {}",
        loaded.scenario.canvas.fps.frame_count(program.duration)
    );
    0
}
