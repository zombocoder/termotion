//! Wires `schema::load`'s output through `timeline::compile` and `Runtime`,
//! `render::CpuRenderer`, and `encode::Encoder` into one streaming render.
//!
//! This lives in the CLI, not a shared crate, because it is the only crate that
//! may depend on `timeline`, `render`, and `encode` all at once (spec's layering:
//! those crates deliberately do not depend on each other or on `schema`). If a
//! future GUI needs the same wiring, extract it then; inventing a crate for one
//! caller now would be speculative.

// `Diagnostic` (~136 bytes) is one of `PipelineError`'s variants and is returned
// by value throughout `termotion-schema` and `termotion-timeline` (ruling R8);
// boxing just this one signature would make the error type inconsistent with the
// rest of the pipeline for no runtime gain.
#![allow(clippy::result_large_err)]

use termotion_core::{Frame, Time};
use termotion_encode::{encoder_for, EncodeError};
use termotion_render::{grid_for, CpuRenderer, FontStack, RenderError, Renderer};
use termotion_schema::diag::Diagnostic;
use termotion_schema::resolve::Loaded;
use termotion_timeline::{compile, Runtime};

/// Frame count and total duration of a completed render, reported back to the
/// caller for the human-readable summary line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSummary {
    pub frames: u64,
    pub duration: Time,
}

/// Everything that can go wrong once a scenario has already loaded and resolved
/// successfully. Each variant maps to a stage of the pipeline, so the CLI can
/// translate it into the right diagnostic and exit code.
#[derive(Debug)]
pub enum PipelineError {
    Compile(Diagnostic),
    Render(RenderError),
    Encode(EncodeError),
}

/// A scenario with zero timeline actions still has a duration of zero; render at
/// least one frame so `termotion render` on an empty scenario produces a single
/// frame of the initial (blank) state rather than an empty output directory.
const MINIMUM_FRAMES: u64 = 1;

/// Renders every frame of `loaded` and streams each one to the appropriate
/// encoder, reporting `(frames_done, frames_total)` after each frame.
///
/// Exactly one `Frame` buffer is allocated and reused for the whole render, so
/// peak memory is independent of frame count: a 30-second 1080p60
/// render pushes ~1,800 frames through the same ~8MB buffer rather than
/// allocating ~14GB up front.
pub fn render_to(
    loaded: &Loaded,
    mut progress: impl FnMut(u64, u64),
) -> Result<RenderSummary, PipelineError> {
    let scenario = &loaded.scenario;

    // Real shaped metrics supersede `GridSpec::estimate`, which exists only for
    // pre-font commands (`validate`, `inspect`) that must not pay for a font load.
    let font = FontStack::load(&scenario.font).map_err(|err| PipelineError::Render(err.into()))?;
    let grid = grid_for(&font, &scenario.terminal);
    drop(font);

    let program = compile(scenario, grid).map_err(PipelineError::Compile)?;
    let duration = program.duration;
    let mut runtime = Runtime::new(program);

    let fps = loaded.output.fps;
    let total = fps.frame_count(duration).max(MINIMUM_FRAMES);

    // `CpuRenderer::new` loads its own `FontStack`; the one above exists only to
    // derive `grid` before `compile` needs it, so this is a second, one-time load
    // rather than a per-frame cost.
    let mut renderer = CpuRenderer::new(scenario).map_err(PipelineError::Render)?;
    let mut encoder = encoder_for(&loaded.output).map_err(PipelineError::Encode)?;
    encoder
        .begin(&loaded.output)
        .map_err(PipelineError::Encode)?;

    let mut frame = Frame::new(loaded.output.size.width, loaded.output.size.height);

    for index in 0..total {
        let t = fps.frame_time(index);
        let state = runtime.state_at(t);
        renderer
            .render(state, t, &mut frame)
            .map_err(PipelineError::Render)?;
        encoder.push_frame(&frame).map_err(PipelineError::Encode)?;
        progress(index + 1, total);
    }

    encoder.finish().map_err(PipelineError::Encode)?;

    Ok(RenderSummary {
        frames: total,
        duration,
    })
}
