use smol_str::SmolStr;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::grid::{Cell, Grid, Overflow};
use crate::style::{StyleId, StyleTable};
use crate::time::Time;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

/// Default cursor blink half-period. A 500ms half-period gives a 1s on/off
/// cycle, which reads as a terminal cursor rather than a strobe.
///
/// Referenced from `CursorConfig::default` (`config.rs`) and the scenario
/// resolver (`termotion-schema`) as well, so all three agree by construction
/// instead of independently repeating the literal.
pub const DEFAULT_CURSOR_BLINK: Time = Time::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorState {
    /// Row index, normally in `[0, grid.rows())`.
    ///
    /// **This invariant can break.** Under `Overflow::Clip` / `Overflow::Error`,
    /// once content overflows the last row this is parked at exactly
    /// `grid.rows()` — one past the end — as a sentinel meaning "not writable"
    /// (see `TerminalState::newline`). Consumers MUST bounds-check `row` against
    /// `grid.rows()` before using it as a grid index or a draw position; indexing
    /// or painting it unchecked will panic or draw outside the grid.
    pub row: u16,
    /// Column index, normally in `[0, grid.cols())`.
    ///
    /// **This invariant can break.** `col` may legitimately equal `grid.cols()`
    /// — one past the last column — meaning the cursor sits just past the edge
    /// (e.g. immediately after a write that exactly fills the row, or while
    /// parked past the end under `Overflow::Clip` / `Overflow::Error`, see
    /// `row` above). Consumers MUST bounds-check `col` against `grid.cols()`
    /// before using it as a grid index or a draw position.
    pub col: u16,
    pub visible: bool,
    pub style: CursorStyle,
    /// Half-period of the blink cycle. `None` means a steady, non-blinking cursor.
    pub blink: Option<Time>,
}

impl Default for CursorState {
    fn default() -> Self {
        CursorState {
            row: 0,
            col: 0,
            visible: true,
            style: CursorStyle::Block,
            blink: Some(DEFAULT_CURSOR_BLINK),
        }
    }
}

/// The complete terminal contents at one instant of virtual time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalState {
    pub grid: Grid,
    pub cursor: CursorState,
    pub styles: StyleTable,
    /// Style applied to subsequent writes (`set_color` / `reset_style`).
    pub current_style: StyleId,
    pub overflow: Overflow,
    /// How many times the grid has scrolled. Useful for diagnostics and tests.
    pub scrolled: u64,
}

impl TerminalState {
    pub fn new(cols: u16, rows: u16, overflow: Overflow) -> Self {
        TerminalState {
            grid: Grid::new(cols, rows),
            cursor: CursorState::default(),
            styles: StyleTable::new(),
            current_style: StyleId::DEFAULT,
            overflow,
            scrolled: 0,
        }
    }

    pub fn set_style(&mut self, style: StyleId) {
        self.current_style = style;
    }

    /// Writes one grapheme cluster at the cursor, wrapping and scrolling as needed.
    pub fn put_grapheme(&mut self, grapheme: &str) {
        let width = grapheme.width().clamp(0, 2) as u8;
        let width = if width == 0 { 1 } else { width }; // control chars occupy a cell
        let cols = self.grid.cols();

        if self.cursor.col + u16::from(width) > cols {
            self.newline();
        }
        if !self.row_is_writable() {
            return;
        }

        let style = self.current_style;
        let col = self.cursor.col;
        let row_index = self.cursor.row;

        if let Some(row) = self.grid.row_mut(row_index) {
            if let Some(cell) = row.cells.get_mut(col as usize) {
                *cell = Cell {
                    g: SmolStr::new(grapheme),
                    width,
                    style,
                };
            }
            if width == 2 {
                if let Some(cont) = row.cells.get_mut(col as usize + 1) {
                    *cont = Cell {
                        g: SmolStr::new_inline(""),
                        width: 0,
                        style,
                    };
                }
            }
            row.dirty = true;
        }

        self.cursor.col = (col + u16::from(width)).min(cols);
    }

    pub fn newline(&mut self) {
        self.cursor.col = 0;
        let last = self.grid.rows().saturating_sub(1);
        if self.cursor.row < last {
            self.cursor.row += 1;
            return;
        }
        match self.overflow {
            // `Error` is rejected at compile time; behave like `Clip` if reached.
            //
            // Deviation from the brief: the brief's draft clamped `cursor.row` back
            // to `last` here, which let a second overflow-newline overwrite the
            // last row's existing content instead of dropping the write (test
            // `clip_overflow_drops_writes_past_the_last_row` catches this). Moving
            // the cursor one row past the end instead makes `row_is_writable`
            // return false, so subsequent `put_grapheme` calls are no-ops and the
            // last row's content is preserved, matching "clip" semantics.
            //
            // This intentionally parks `cursor.row == grid.rows()`, breaking the
            // usual `[0, rows())` invariant on the field — see the doc comment on
            // `CursorState::row` for what consumers must do about it.
            Overflow::Clip | Overflow::Error => {
                self.cursor.row = self.grid.rows();
            }
            Overflow::Scroll => {
                self.grid.scroll_up(1);
                self.scrolled += 1;
                self.cursor.row = last;
            }
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor.col == 0 {
            return;
        }
        let row_index = self.cursor.row;
        let mut target = self.cursor.col - 1;

        if let Some(row) = self.grid.row(row_index) {
            // Stepping back onto a continuation cell means the real glyph is one
            // column further left; erase the pair together.
            if row.cells.get(target as usize).map(|c| c.width) == Some(0) && target > 0 {
                target -= 1;
            }
        }

        if let Some(row) = self.grid.row_mut(row_index) {
            let span = row.cells.get(target as usize).map(|c| c.width).unwrap_or(1);
            let span = if span == 2 { 2 } else { 1 };
            for offset in 0..span {
                if let Some(cell) = row.cells.get_mut(target as usize + offset as usize) {
                    *cell = Cell::blank();
                }
            }
            row.dirty = true;
        }

        self.cursor.col = target;
    }

    pub fn clear(&mut self) {
        self.grid.clear();
        self.cursor.row = 0;
        self.cursor.col = 0;
    }

    /// Replaces the entire contents of `row` with the given styled spans, tagging
    /// it with `id`. Out-of-range rows are ignored rather than panicking.
    pub fn write_row(&mut self, row_index: u16, spans: &[(String, StyleId)], id: Option<SmolStr>) {
        let cols = self.grid.cols();
        let Some(row) = self.grid.row_mut(row_index) else {
            return;
        };

        row.reset();
        row.id = id;

        let mut col: u16 = 0;
        for (text, style) in spans {
            for grapheme in text.graphemes(true) {
                let width = grapheme.width().clamp(0, 2) as u8;
                let width = if width == 0 { 1 } else { width };
                if col + u16::from(width) > cols {
                    break;
                }
                if let Some(cell) = row.cells.get_mut(col as usize) {
                    *cell = Cell {
                        g: SmolStr::new(grapheme),
                        width,
                        style: *style,
                    };
                }
                if width == 2 {
                    if let Some(cont) = row.cells.get_mut(col as usize + 1) {
                        *cont = Cell {
                            g: SmolStr::new_inline(""),
                            width: 0,
                            style: *style,
                        };
                    }
                }
                col += u16::from(width);
            }
        }
        row.dirty = true;
    }

    fn row_is_writable(&self) -> bool {
        self.cursor.row < self.grid.rows()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Overflow;

    fn text_of_row(state: &TerminalState, row: u16) -> String {
        state
            .grid
            .row(row)
            .map(|r| r.cells.iter().map(|c| c.g.as_str()).collect::<String>())
            .unwrap_or_default()
            .trim_end()
            .to_string()
    }

    fn write(state: &mut TerminalState, text: &str) {
        for g in unicode_segmentation::UnicodeSegmentation::graphemes(text, true) {
            state.put_grapheme(g);
        }
    }

    #[test]
    fn writes_advance_the_cursor_one_column_per_grapheme() {
        let mut state = TerminalState::new(20, 4, Overflow::Scroll);
        write(&mut state, "hi");
        assert_eq!(text_of_row(&state, 0), "hi");
        assert_eq!((state.cursor.row, state.cursor.col), (0, 2));
    }

    #[test]
    fn combining_marks_occupy_a_single_cell() {
        let mut state = TerminalState::new(20, 4, Overflow::Scroll);
        write(&mut state, "e\u{0301}x"); // "é" as e + combining acute, then x
        assert_eq!(state.cursor.col, 2);
        assert_eq!(state.grid.row(0).unwrap().cells[1].g.as_str(), "x");
    }

    #[test]
    fn wide_glyphs_take_two_cells_with_a_zero_width_continuation() {
        let mut state = TerminalState::new(20, 4, Overflow::Scroll);
        write(&mut state, "世a");
        let cells = &state.grid.row(0).unwrap().cells;
        assert_eq!(cells[0].width, 2);
        assert_eq!(cells[1].width, 0);
        assert_eq!(cells[2].g.as_str(), "a");
        assert_eq!(state.cursor.col, 3);
    }

    #[test]
    fn wide_glyph_with_exactly_one_column_left_wraps_instead_of_splitting() {
        // cols = 3: after "ab" only 1 column remains, not enough for a 2-wide
        // glyph. It must wrap whole to the next row, never split across rows
        // and never silently clipped into the single remaining column.
        let mut state = TerminalState::new(3, 4, Overflow::Scroll);
        write(&mut state, "ab");
        assert_eq!(state.cursor.col, 2);
        write(&mut state, "世");
        assert_eq!(text_of_row(&state, 0), "ab");
        let cells = &state.grid.row(1).unwrap().cells;
        assert_eq!(cells[0].g.as_str(), "世");
        assert_eq!(cells[0].width, 2);
        assert_eq!(cells[1].width, 0);
        assert_eq!((state.cursor.row, state.cursor.col), (1, 2));
    }

    #[test]
    fn writing_past_the_last_column_wraps_to_the_next_row() {
        let mut state = TerminalState::new(3, 4, Overflow::Scroll);
        write(&mut state, "abcd");
        assert_eq!(text_of_row(&state, 0), "abc");
        assert_eq!(text_of_row(&state, 1), "d");
        assert_eq!((state.cursor.row, state.cursor.col), (1, 1));
    }

    #[test]
    fn newline_returns_to_column_zero_on_the_next_row() {
        let mut state = TerminalState::new(10, 4, Overflow::Scroll);
        write(&mut state, "ab");
        state.newline();
        assert_eq!((state.cursor.row, state.cursor.col), (1, 0));
    }

    #[test]
    fn scroll_overflow_shifts_content_up_and_counts_scrolls() {
        let mut state = TerminalState::new(10, 2, Overflow::Scroll);
        write(&mut state, "one");
        state.newline();
        write(&mut state, "two");
        state.newline();
        write(&mut state, "three");
        assert_eq!(text_of_row(&state, 0), "two");
        assert_eq!(text_of_row(&state, 1), "three");
        assert_eq!(state.cursor.row, 1);
        assert_eq!(state.scrolled, 1);
    }

    #[test]
    fn clip_overflow_drops_writes_past_the_last_row() {
        let mut state = TerminalState::new(10, 2, Overflow::Clip);
        write(&mut state, "one");
        state.newline();
        write(&mut state, "two");
        state.newline();
        write(&mut state, "three");
        assert_eq!(text_of_row(&state, 0), "one");
        assert_eq!(text_of_row(&state, 1), "two");
        assert_eq!(state.scrolled, 0);
    }

    #[test]
    fn backspace_erases_the_previous_cell() {
        let mut state = TerminalState::new(10, 2, Overflow::Scroll);
        write(&mut state, "abc");
        state.backspace();
        assert_eq!(text_of_row(&state, 0), "ab");
        assert_eq!(state.cursor.col, 2);
    }

    #[test]
    fn backspace_over_a_wide_glyph_erases_both_cells() {
        let mut state = TerminalState::new(10, 2, Overflow::Scroll);
        write(&mut state, "世");
        state.backspace();
        assert_eq!(state.cursor.col, 0);
        assert!(state.grid.row(0).unwrap().cells[0].is_blank());
        assert_eq!(state.grid.row(0).unwrap().cells[1].width, 1);
    }

    #[test]
    fn backspace_at_column_zero_is_a_no_op() {
        let mut state = TerminalState::new(10, 2, Overflow::Scroll);
        state.backspace();
        assert_eq!((state.cursor.row, state.cursor.col), (0, 0));
    }

    #[test]
    fn clear_blanks_the_grid_and_homes_the_cursor() {
        let mut state = TerminalState::new(10, 2, Overflow::Scroll);
        write(&mut state, "abc");
        state.clear();
        assert_eq!(text_of_row(&state, 0), "");
        assert_eq!((state.cursor.row, state.cursor.col), (0, 0));
    }

    #[test]
    fn write_row_replaces_a_row_and_tags_it_with_an_id() {
        let mut state = TerminalState::new(20, 3, Overflow::Scroll);
        let style = state.styles.intern(crate::style::TextStyle::default());
        state.write_row(
            1,
            &[
                ("[ .. ] ".to_string(), style),
                ("camera".to_string(), style),
            ],
            Some(smol_str::SmolStr::new("camera")),
        );
        assert_eq!(text_of_row(&state, 1), "[ .. ] camera");
        assert_eq!(state.grid.find_row_by_id("camera"), Some(1));
    }

    #[test]
    fn write_row_out_of_range_is_ignored() {
        let mut state = TerminalState::new(20, 2, Overflow::Scroll);
        state.write_row(99, &[("x".to_string(), StyleId::DEFAULT)], None);
        assert_eq!(text_of_row(&state, 0), "");
    }
}
