use std::path::Path;

use termotion_render::{grid_for, FontStack};
use termotion_schema::resolve::Overrides;
use termotion_timeline::compile;

use crate::report;

/// Exit codes follow the documented exit-code table; the caller passes this
/// straight to `process::exit`.
pub fn run(path: &Path, json: bool) -> i32 {
    let loaded = match termotion_schema::load(path, &Overrides::default()) {
        Ok(loaded) => loaded,
        Err(errors) => return fail(&errors, json),
    };

    // The font is loaded FIRST, and the grid derived from its real shaped
    // metrics via `grid_for` — the same path `render` takes — so `validate`
    // reports the grid (and any wrap/scroll behavior implied by it) that
    // will actually render, instead of `GridSpec::estimate`'s fixed 0.6
    // advance-ratio guess, which only agrees with real metrics for the
    // embedded default font.
    let font = match FontStack::load(&loaded.scenario.font) {
        Ok(font) => font,
        Err(err) => {
            let diagnostic = termotion_schema::diag::Diagnostic::error(
                termotion_schema::diag::codes::FONT_NOT_FOUND,
                err.to_string(),
            )
            .at_path("font.family")
            .in_file(path)
            .with_hint("Install the font or specify:\n\nfont:\n  path: ./assets/fonts/font.ttf");
            return fail(&[diagnostic], json);
        }
    };
    let grid = grid_for(&font, &loaded.scenario.terminal);
    drop(font);

    let program = match compile(&loaded.scenario, grid) {
        Ok(program) => program,
        Err(diagnostic) => return fail(&[diagnostic.in_file(path)], json),
    };

    let frames = loaded.scenario.canvas.fps.frame_count(program.duration);

    if json {
        let payload = serde_json::json!({
            "valid": true,
            "actions": loaded.scenario.timeline.len(),
            "events": program.events.len(),
            "duration_ms": program.duration.as_millis(),
            "frames": frames,
            "width": loaded.scenario.canvas.size.width,
            "height": loaded.scenario.canvas.size.height,
            // A plain `num() / den()` integer division would report 30000/1001
            // (NTSC 29.97) as a flatly wrong 29; dividing as f64 keeps this
            // machine-readable field accurate for non-integer frame rates.
            "fps": f64::from(loaded.scenario.canvas.fps.num())
                / f64::from(loaded.scenario.canvas.fps.den()),
        });
        println!("{payload}");
    } else {
        println!("✓ syntax");
        println!("✓ schema");
        println!("✓ timeline");
        println!("✓ theme");
        println!("✓ font");
        println!("✓ output configuration");
        println!();
        println!("Scenario valid.");
        println!("Duration: {:.2}s", program.duration.as_secs_f64());
        println!("Frames: {frames}");
    }
    0
}

fn fail(errors: &[termotion_schema::diag::Diagnostic], json: bool) -> i32 {
    if json {
        let payload = serde_json::json!({
            "valid": false,
            "errors": report::to_json(errors),
        });
        println!("{payload}");
    } else {
        report::print_diagnostics(errors);
    }
    report::exit_code_for(errors)
}
