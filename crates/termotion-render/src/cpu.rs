use termotion_core::{
    BackgroundConfig, Cell, Color, CursorStyle, Frame, Palette, Point, Scenario, Size, StyleTable,
    TerminalState, Time,
};

use crate::cursor;
use crate::font::{FontMetrics, FontStack};
use crate::glyphs::GlyphCache;
use crate::{RenderError, Renderer};

/// Bytes per pixel in `Frame`'s straight RGBA8 buffer. `Frame` keeps this private
/// (it's an implementation detail of the pixel format, not part of its API), so
/// row-level slice writes here mirror it under its own name.
const BYTES_PER_PIXEL: usize = 4;

/// Software renderer: background and text are composed into a persistent `base`
/// frame, with only changed rows repainted between frames.
///
/// Rows are compared by content, not by `Row::dirty`: the animation runtime seeks
/// freely (including backward), and a dirty flag set on the last forward write
/// carries no meaning after a seek. Comparing the previous frame's cells against
/// the current ones is correct regardless of how time moved.
pub struct CpuRenderer {
    font: FontStack,
    glyphs: GlyphCache,
    metrics: FontMetrics,
    palette: Palette,
    canvas: Size,
    origin: Point,
    background: Color,
    transparent: bool,
    cursor_style: CursorStyle,
    base: Frame,
    /// Snapshot of each row's cells as of the last repaint, used to detect which
    /// rows changed. Empty until the first `render` call.
    previous: Vec<Vec<Cell>>,
    initialized: bool,
}

impl CpuRenderer {
    pub fn new(scenario: &Scenario) -> Result<Self, RenderError> {
        let font = FontStack::load(&scenario.font)?;
        let metrics = font.metrics();
        let inner = scenario.terminal.bounds.inset(scenario.terminal.padding);
        // `BackgroundConfig` currently has exactly one variant, so this destructure
        // is irrefutable; `match` would be flagged by clippy as needless.
        let BackgroundConfig::Solid(background) = scenario.canvas.background;

        Ok(CpuRenderer {
            font,
            glyphs: GlyphCache::new(),
            metrics,
            palette: scenario.palette,
            canvas: scenario.canvas.size,
            origin: Point {
                x: inner.x,
                y: inner.y,
            },
            background,
            transparent: scenario.canvas.transparent,
            cursor_style: scenario.cursor.style,
            base: Frame::new(scenario.canvas.size.width, scenario.canvas.size.height),
            previous: Vec::new(),
            initialized: false,
        })
    }

    pub fn metrics(&self) -> FontMetrics {
        self.metrics
    }

    fn paint_background(&mut self) {
        if self.transparent {
            self.base.clear_transparent();
        } else {
            self.base.fill(self.background);
        }
    }
}

/// Clears one row band (`y0..y0 + line_h`, full canvas width) back to the base
/// background, ahead of repainting its glyphs.
fn clear_row_band(base: &mut Frame, y0: i32, line_h: i32, transparent: bool, background: Color) {
    let width = base.width();
    let row_bytes = width as usize * BYTES_PER_PIXEL;

    for y in y0..y0 + line_h {
        if y < 0 || y as u32 >= base.height() {
            continue;
        }
        let start = y as usize * width as usize * BYTES_PER_PIXEL;
        let row = &mut base.data_mut()[start..start + row_bytes];
        if transparent {
            row.fill(0);
        } else {
            for pixel in row.chunks_exact_mut(BYTES_PER_PIXEL) {
                pixel[0] = background.r;
                pixel[1] = background.g;
                pixel[2] = background.b;
                pixel[3] = background.a;
            }
        }
    }
}

/// Everything `paint_row` needs that doesn't change row to row. Bundled into one
/// struct (rather than four extra parameters) to keep `paint_row` under clippy's
/// argument-count lint without reaching for an `#[allow]`.
struct RowPaintContext {
    metrics: FontMetrics,
    origin: Point,
    background: Color,
    transparent: bool,
}

/// Repaints one row band into `base`: background first, then every inked cell.
///
/// A free function rather than a `&mut self` method: it needs `glyphs`, `font`,
/// and `base` mutably at once, which are three distinct fields of `CpuRenderer`.
/// Taking them as separate parameters (as the call site does, passing
/// `&mut self.base`, `&mut self.glyphs`, `&mut self.font`) sidesteps the
/// single-mutable-borrow-of-`self` restriction outright.
///
/// Colors come from the cell's interned `StyleId` via `styles`, so `set_color` and
/// every prompt segment render in their own color rather than a single foreground.
fn paint_row(
    base: &mut Frame,
    glyphs: &mut GlyphCache,
    font: &mut FontStack,
    ctx: &RowPaintContext,
    row_index: u16,
    cells: &[Cell],
    styles: &StyleTable,
) {
    let line_h = ctx.metrics.line_h as i32;
    let advance = ctx.metrics.advance_w as i32;
    let y0 = ctx.origin.y + i32::from(row_index) * line_h;

    clear_row_band(base, y0, line_h, ctx.transparent, ctx.background);

    let baseline = y0 + ctx.metrics.ascent.round() as i32;

    for (col, cell) in cells.iter().enumerate() {
        // Continuation cells (width 0) carry no glyph of their own, and blank
        // cells have nothing to ink; skip both.
        if cell.width == 0 || cell.is_blank() {
            continue;
        }

        let pen_x = ctx.origin.x + col as i32 * advance;
        let color = styles.get(cell.style).fg;

        let Some(mask) = glyphs.mask(font, cell.g.as_str()) else {
            continue;
        };
        if mask.width == 0 || mask.height == 0 {
            continue;
        }

        let mask_x = pen_x + mask.left;
        let mask_y = baseline - mask.top;
        for my in 0..mask.height {
            for mx in 0..mask.width {
                let coverage = mask.alpha[(my * mask.width + mx) as usize];
                if coverage > 0 {
                    base.blend(mask_x + mx as i32, mask_y + my as i32, color, coverage);
                }
            }
        }
    }
}

impl Renderer for CpuRenderer {
    fn resize(&mut self, canvas: Size) -> Result<(), RenderError> {
        self.canvas = canvas;
        self.base = Frame::new(canvas.width, canvas.height);
        self.previous.clear();
        self.initialized = false;
        Ok(())
    }

    fn render(
        &mut self,
        state: &TerminalState,
        _t: Time,
        into: &mut Frame,
    ) -> Result<(), RenderError> {
        if into.width() != self.canvas.width || into.height() != self.canvas.height {
            return Err(RenderError::SizeMismatch {
                expected: self.canvas,
                got: Size {
                    width: into.width(),
                    height: into.height(),
                },
            });
        }

        if !self.initialized {
            self.paint_background();
            self.initialized = true;
        }

        // Keep `previous` sized to the current grid so a mismatched length (e.g. a
        // renderer reused across differently-sized states) forces every row to be
        // treated as changed rather than panicking or comparing stale rows.
        let rows = state.grid.rows() as usize;
        if self.previous.len() != rows {
            self.previous = vec![Vec::new(); rows];
        }

        let row_ctx = RowPaintContext {
            metrics: self.metrics,
            origin: self.origin,
            background: self.background,
            transparent: self.transparent,
        };

        for row_index in 0..state.grid.rows() {
            let Some(row) = state.grid.row(row_index) else {
                continue;
            };
            let changed = self.previous[row_index as usize].as_slice() != row.cells.as_slice();
            if changed {
                paint_row(
                    &mut self.base,
                    &mut self.glyphs,
                    &mut self.font,
                    &row_ctx,
                    row_index,
                    &row.cells,
                    &state.styles,
                );
                self.previous[row_index as usize] = row.cells.clone();
            }
        }

        into.data_mut().copy_from_slice(self.base.data());

        // R6: under `Overflow::Clip` / `Overflow::Error`, the cursor can be parked
        // one past the last row (`row == grid.rows()`) or column (`col ==
        // grid.cols()`) as a "not writable" sentinel — see `CursorState`'s field
        // docs. Drawing unconditionally on `visible` would paint a cursor cell
        // outside the terminal region in that case, so both bounds are checked
        // here before handing off to the cursor painter.
        //
        // Only `visible` is read, never `blink`: the animation runtime already
        // folds blink phase into `visible` every frame and does not keep `blink`
        // in sync with it, so `blink` can be stale by the time it reaches here.
        let cursor = &state.cursor;
        if cursor.visible && cursor.row < state.grid.rows() && cursor.col < state.grid.cols() {
            cursor::draw(
                into,
                self.metrics,
                self.origin,
                cursor.row,
                cursor.col,
                self.cursor_style,
                self.palette.cursor,
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termotion_core::{
        BackgroundConfig, Color, CursorStyle, Frame, Overflow, Padding, Rect, Scenario, Size,
        TerminalState, Time,
    };

    fn scenario() -> Scenario {
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
            width: 400,
            height: 200,
        };
        scenario.canvas.background = BackgroundConfig::Solid(Color::rgb(8, 11, 9));
        scenario.terminal.bounds = Rect {
            x: 0,
            y: 0,
            width: 400,
            height: 200,
        };
        scenario.terminal.padding = Padding::uniform(0);
        scenario.font.size = 20.0;
        scenario
    }

    fn state_with(text: &str) -> TerminalState {
        let mut state = TerminalState::new(30, 7, Overflow::Scroll);
        for grapheme in text.chars() {
            state.put_grapheme(&grapheme.to_string());
        }
        state.cursor.visible = false;
        state
    }

    fn render(renderer: &mut CpuRenderer, state: &TerminalState) -> Frame {
        let mut frame = Frame::new(400, 200);
        renderer.render(state, Time::ZERO, &mut frame).unwrap();
        frame
    }

    #[test]
    fn fills_the_canvas_with_the_background_color() {
        let mut renderer = CpuRenderer::new(&scenario()).unwrap();
        let frame = render(&mut renderer, &state_with(""));
        assert_eq!(frame.pixel(399, 199), Color::rgb(8, 11, 9));
        assert_eq!(frame.pixel(0, 0), Color::rgb(8, 11, 9));
    }

    #[test]
    fn transparent_canvases_leave_alpha_at_zero() {
        let mut scenario = scenario();
        scenario.canvas.transparent = true;
        let mut renderer = CpuRenderer::new(&scenario).unwrap();
        let frame = render(&mut renderer, &state_with(""));
        assert_eq!(frame.pixel(399, 199).a, 0);
    }

    #[test]
    fn drawn_text_differs_from_the_background() {
        let mut renderer = CpuRenderer::new(&scenario()).unwrap();
        let blank = render(&mut renderer, &state_with(""));

        let mut renderer = CpuRenderer::new(&scenario()).unwrap();
        let written = render(&mut renderer, &state_with("W"));

        assert_ne!(blank.data(), written.data(), "text must change the frame");
    }

    #[test]
    fn text_is_drawn_inside_the_first_cell() {
        let mut renderer = CpuRenderer::new(&scenario()).unwrap();
        let frame = render(&mut renderer, &state_with("W"));

        let metrics = renderer.metrics();
        let mut inked = false;
        for y in 0..metrics.line_h {
            for x in 0..metrics.advance_w {
                if frame.pixel(x, y) != Color::rgb(8, 11, 9) {
                    inked = true;
                }
            }
        }
        assert!(inked, "no ink found in the first cell");
    }

    #[test]
    fn rendering_the_same_state_twice_produces_identical_frames() {
        let mut renderer = CpuRenderer::new(&scenario()).unwrap();
        let state = state_with("hello");
        let first = render(&mut renderer, &state);
        let second = render(&mut renderer, &state);
        assert_eq!(first.data(), second.data());
    }

    #[test]
    fn two_renderers_agree_on_the_same_state() {
        let state = state_with("hello world");
        let mut a = CpuRenderer::new(&scenario()).unwrap();
        let mut b = CpuRenderer::new(&scenario()).unwrap();
        assert_eq!(render(&mut a, &state).data(), render(&mut b, &state).data());
    }

    #[test]
    fn incremental_updates_match_a_fresh_render() {
        // The dirty-row optimization must not drift from a from-scratch render.
        let mut incremental = CpuRenderer::new(&scenario()).unwrap();
        render(&mut incremental, &state_with("a"));
        render(&mut incremental, &state_with("ab"));
        let stepped = render(&mut incremental, &state_with("abc"));

        let mut fresh = CpuRenderer::new(&scenario()).unwrap();
        let direct = render(&mut fresh, &state_with("abc"));

        assert_eq!(stepped.data(), direct.data());
    }

    #[test]
    fn erased_text_is_removed_from_the_frame() {
        let mut renderer = CpuRenderer::new(&scenario()).unwrap();
        let blank = render(&mut renderer, &state_with(""));
        render(&mut renderer, &state_with("xyz"));

        let mut erased = state_with("xyz");
        erased.backspace();
        erased.backspace();
        erased.backspace();
        let after = render(&mut renderer, &erased);

        assert_eq!(after.data(), blank.data());
    }

    #[test]
    fn a_visible_block_cursor_marks_its_cell() {
        let mut scenario = scenario();
        scenario.cursor.style = CursorStyle::Block;
        let mut renderer = CpuRenderer::new(&scenario).unwrap();

        let mut hidden = state_with("");
        hidden.cursor.visible = false;
        let without = render(&mut renderer, &hidden);

        let mut shown = state_with("");
        shown.cursor.visible = true;
        let with = render(&mut renderer, &shown);

        assert_ne!(without.data(), with.data(), "cursor must be visible");
    }

    #[test]
    fn cursor_styles_render_differently() {
        let mut block_scenario = scenario();
        block_scenario.cursor.style = CursorStyle::Block;
        let mut bar_scenario = scenario();
        bar_scenario.cursor.style = CursorStyle::Bar;

        let mut state = state_with("");
        state.cursor.visible = true;

        let mut block = CpuRenderer::new(&block_scenario).unwrap();
        let mut bar = CpuRenderer::new(&bar_scenario).unwrap();
        assert_ne!(
            render(&mut block, &state).data(),
            render(&mut bar, &state).data()
        );
    }

    #[test]
    fn a_frame_of_the_wrong_size_is_rejected() {
        let mut renderer = CpuRenderer::new(&scenario()).unwrap();
        let mut frame = Frame::new(100, 100);
        let err = renderer
            .render(&state_with(""), Time::ZERO, &mut frame)
            .unwrap_err();
        assert!(matches!(err, RenderError::SizeMismatch { .. }));
    }

    /// Mandatory correction (ruling R6): under `Overflow::Clip` / `Overflow::Error`,
    /// `TerminalState` parks the cursor one past the last row as a "not writable"
    /// sentinel. `cursor.row == grid.rows()` there, so drawing on `visible` alone
    /// would paint a cursor band one line below the terminal region. A clipped
    /// state whose cursor sits out of range must render identically to the same
    /// state with the cursor hidden.
    #[test]
    fn an_out_of_range_cursor_is_not_drawn() {
        let mut scenario = scenario();
        scenario.cursor.style = CursorStyle::Block;
        scenario.terminal.overflow = Overflow::Clip;

        // Two rows, wide enough that a couple of `newline`s past the end park the
        // cursor at `row == grid.rows()` under `Overflow::Clip`.
        let mut out_of_range = TerminalState::new(10, 2, Overflow::Clip);
        out_of_range.newline();
        out_of_range.newline();
        out_of_range.cursor.visible = true;
        assert_eq!(out_of_range.cursor.row, out_of_range.grid.rows());

        let mut hidden = TerminalState::new(10, 2, Overflow::Clip);
        hidden.newline();
        hidden.newline();
        hidden.cursor.visible = false;
        assert_eq!(hidden.cursor.row, out_of_range.cursor.row);

        let mut renderer = CpuRenderer::new(&scenario).unwrap();
        let with_out_of_range_cursor = render(&mut renderer, &out_of_range);

        let mut renderer = CpuRenderer::new(&scenario).unwrap();
        let with_hidden_cursor = render(&mut renderer, &hidden);

        assert_eq!(
            with_out_of_range_cursor.data(),
            with_hidden_cursor.data(),
            "an out-of-range cursor must not be painted"
        );
    }
}
