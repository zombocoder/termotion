use termotion_core::{Color, CursorStyle, Frame, Point};

use crate::font::FontMetrics;

/// Thickness of the `Underline` and `Bar` cursor styles, as a fraction of line
/// height. 8% reads as a crisp stroke without looking like a second baseline.
const CURSOR_THICKNESS_RATIO: f32 = 0.08;

/// Floor on cursor stroke thickness, in pixels, so small font sizes still render
/// a visible underline/bar instead of rounding down to a sliver or nothing.
const CURSOR_MIN_THICKNESS_PX: f32 = 2.0;

/// Coverage used for the cursor fill: the cursor is a solid shape, never
/// antialiased against its background.
const CURSOR_COVERAGE: u8 = 255;

/// Draws the cursor at a cell. `Block` uses inverse video: the cell is filled with
/// the cursor color and the glyph beneath is not redrawn, which reads correctly for
/// a trailing cursor on an empty cell and is the conventional terminal look.
///
/// Callers must bounds-check `row`/`col` against the grid before calling this:
/// `TerminalState::cursor` can legitimately be parked one past the last row/column
/// as a "not writable" sentinel, and drawing at that position would paint outside
/// the terminal region.
pub fn draw(
    frame: &mut Frame,
    metrics: FontMetrics,
    origin: Point,
    row: u16,
    col: u16,
    style: CursorStyle,
    color: Color,
) {
    let x0 = origin.x + i32::from(col) * metrics.advance_w as i32;
    let y0 = origin.y + i32::from(row) * metrics.line_h as i32;
    let w = metrics.advance_w as i32;
    let h = metrics.line_h as i32;
    let thickness = ((metrics.line_h as f32) * CURSOR_THICKNESS_RATIO)
        .round()
        .max(CURSOR_MIN_THICKNESS_PX) as i32;

    let (rx, ry, rw, rh) = match style {
        CursorStyle::Block => (x0, y0, w, h),
        CursorStyle::Underline => (x0, y0 + h - thickness, w, thickness),
        CursorStyle::Bar => (x0, y0, thickness, h),
    };

    for y in ry..ry + rh {
        for x in rx..rx + rw {
            frame.blend(x, y, color, CURSOR_COVERAGE);
        }
    }
}
