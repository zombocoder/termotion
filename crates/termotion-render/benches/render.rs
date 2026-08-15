use std::cell::Cell;

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use termotion_core::{
    BackgroundConfig, Color, Frame, Overflow, Rect, Scenario, Size, TerminalState, Time,
};
use termotion_render::{CpuRenderer, Renderer};

fn scenario_1080p() -> Scenario {
    let mut scenario = Scenario {
        metadata: Default::default(),
        canvas: Default::default(),
        terminal: Default::default(),
        font: Default::default(),
        prompt: Default::default(),
        cursor: Default::default(),
        palette: Default::default(),
        playback: Default::default(),
        seed: 0,
        timeline: Vec::new(),
    };
    scenario.canvas.size = Size {
        width: 1920,
        height: 1080,
    };
    scenario.canvas.background = BackgroundConfig::Solid(Color::rgb(8, 11, 9));
    scenario.terminal.bounds = Rect {
        x: 110,
        y: 110,
        width: 1700,
        height: 860,
    };
    scenario
}

fn typing_state(chars: usize) -> TerminalState {
    let mut state = TerminalState::new(70, 15, Overflow::Scroll);
    for i in 0..chars {
        state.put_grapheme(&((b'a' + (i % 26) as u8) as char).to_string());
    }
    state
}

/// Measures `CpuRenderer::render()` alone. `typing_state` rebuilds a
/// `TerminalState` from scratch (up to ~899 `put_grapheme` calls plus an
/// allocation), which has nothing to do with rendering, so it must run in
/// `iter_batched`'s setup closure rather than inside the timed routine — a
/// fix for a Task 23 review finding where an earlier version called it
/// inside `b.iter`, inflating the reported per-frame figure with state
/// construction cost.
///
/// `chars` still varies per batch (cycling 0..900 via a shared `Cell`) so
/// each timed call sees genuinely different content from the last, keeping
/// `CpuRenderer`'s dirty-row cache exercised the way a scenario with active
/// typing would exercise it. A fixed, identical `TerminalState` every batch
/// would make every call after the first a no-op repaint (the dirty-row diff
/// would find nothing changed), which would understate glyph rasterization
/// cost rather than isolating it.
fn bench_1080p30(c: &mut Criterion) {
    let scenario = scenario_1080p();
    let mut renderer = CpuRenderer::new(&scenario).unwrap();
    let mut frame = Frame::new(1920, 1080);
    let chars = Cell::new(0usize);

    c.bench_function("1080p typing frame", |b| {
        b.iter_batched(
            || {
                let n = chars.get();
                chars.set((n + 1) % 900);
                typing_state(n)
            },
            |state| {
                renderer.render(&state, Time::ZERO, &mut frame).unwrap();
            },
            BatchSize::SmallInput,
        )
    });
}

/// Isolates the `base` -> output-`Frame` full-canvas copy that `CpuRenderer::render`
/// performs every frame regardless of how many rows changed (`cpu.rs`'s
/// `into.data_mut().copy_from_slice(self.base.data())`). Task 23's cost
/// attribution needs this split out from `bench_1080p30` to tell a
/// memory-bandwidth-bound renderer from a CPU-work-bound one.
fn bench_frame_copy(c: &mut Criterion) {
    let source = Frame::new(1920, 1080);
    let mut dest = Frame::new(1920, 1080);

    c.bench_function("1080p base->frame copy", |b| {
        b.iter(|| {
            dest.data_mut().copy_from_slice(source.data());
        })
    });
}

criterion_group!(benches, bench_1080p30, bench_frame_copy);
criterion_main!(benches);
