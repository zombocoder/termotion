use smol_str::SmolStr;

use crate::style::StyleId;

/// What happens when content runs past the last row.
///
/// `Error` is detected at compile time; at runtime it behaves like `Clip`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overflow {
    Clip,
    #[default]
    Scroll,
    Error,
}

/// One grid cell. `g` holds a whole grapheme cluster, never a raw `char`.
///
/// `width` is 1 for normal cells, 2 for the leading cell of a wide glyph, and 0
/// for the continuation cell that follows a wide glyph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub g: SmolStr,
    pub width: u8,
    pub style: StyleId,
}

impl Cell {
    pub fn blank() -> Self {
        Cell {
            g: SmolStr::new_inline(" "),
            width: 1,
            style: StyleId::DEFAULT,
        }
    }

    pub fn is_blank(&self) -> bool {
        self.g.as_str() == " "
    }
}

impl Default for Cell {
    fn default() -> Self {
        Cell::blank()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Set by `status`-style actions so a later update can find this row again.
    pub id: Option<SmolStr>,
    pub cells: Vec<Cell>,
    /// Set whenever the row changes; the renderer clears it after re-blitting.
    pub dirty: bool,
}

impl Row {
    pub fn blank(cols: u16) -> Self {
        Row {
            id: None,
            cells: vec![Cell::blank(); cols as usize],
            dirty: true,
        }
    }

    pub fn reset(&mut self) {
        for cell in &mut self.cells {
            *cell = Cell::blank();
        }
        self.id = None;
        self.dirty = true;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    cols: u16,
    rows: u16,
    rows_buf: Vec<Row>,
}

impl Grid {
    pub fn new(cols: u16, rows: u16) -> Self {
        Grid {
            cols,
            rows,
            rows_buf: (0..rows).map(|_| Row::blank(cols)).collect(),
        }
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    pub fn row(&self, index: u16) -> Option<&Row> {
        self.rows_buf.get(index as usize)
    }

    pub fn row_mut(&mut self, index: u16) -> Option<&mut Row> {
        self.rows_buf.get_mut(index as usize)
    }

    pub fn iter_rows(&self) -> impl Iterator<Item = &Row> {
        self.rows_buf.iter()
    }

    /// Shifts rows up by `count`, appending blank rows at the bottom.
    pub fn scroll_up(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        let count = count.min(self.rows as usize);
        self.rows_buf.drain(0..count);
        for _ in 0..count {
            self.rows_buf.push(Row::blank(self.cols));
        }
        for row in &mut self.rows_buf {
            row.dirty = true;
        }
    }

    pub fn clear(&mut self) {
        for row in &mut self.rows_buf {
            row.reset();
        }
    }

    pub fn find_row_by_id(&self, id: &str) -> Option<u16> {
        self.rows_buf
            .iter()
            .position(|row| row.id.as_deref() == Some(id))
            .and_then(|i| u16::try_from(i).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_grid_is_blank_and_correctly_sized() {
        let grid = Grid::new(80, 24);
        assert_eq!((grid.cols(), grid.rows()), (80, 24));
        let row = grid.row(0).expect("row 0 exists");
        assert_eq!(row.cells.len(), 80);
        assert_eq!(row.cells[0].g.as_str(), " ");
        assert!(row.id.is_none());
    }

    #[test]
    fn out_of_range_rows_return_none_rather_than_panicking() {
        let mut grid = Grid::new(10, 2);
        assert!(grid.row(2).is_none());
        assert!(grid.row_mut(99).is_none());
    }

    #[test]
    fn scrolling_discards_the_top_and_appends_blank_rows() {
        let mut grid = Grid::new(4, 3);
        for (i, id) in ["a", "b", "c"].iter().enumerate() {
            grid.row_mut(i as u16).unwrap().id = Some(SmolStr::new(*id));
        }
        grid.scroll_up(1);
        assert_eq!(grid.row(0).unwrap().id.as_deref(), Some("b"));
        assert_eq!(grid.row(1).unwrap().id.as_deref(), Some("c"));
        assert!(grid.row(2).unwrap().id.is_none());
    }

    #[test]
    fn scrolling_past_the_end_blanks_everything() {
        let mut grid = Grid::new(4, 3);
        grid.row_mut(0).unwrap().id = Some(SmolStr::new("a"));
        grid.scroll_up(10);
        assert!(grid.row(0).unwrap().id.is_none());
        assert_eq!(grid.rows(), 3);
    }

    #[test]
    fn clear_blanks_cells_and_drops_row_ids() {
        let mut grid = Grid::new(4, 2);
        grid.row_mut(0).unwrap().id = Some(SmolStr::new("camera"));
        grid.row_mut(0).unwrap().cells[0].g = SmolStr::new("X");
        grid.clear();
        assert!(grid.row(0).unwrap().id.is_none());
        assert_eq!(grid.row(0).unwrap().cells[0].g.as_str(), " ");
    }

    #[test]
    fn rows_are_findable_by_id_after_scrolling() {
        let mut grid = Grid::new(4, 3);
        grid.row_mut(2).unwrap().id = Some(SmolStr::new("camera"));
        assert_eq!(grid.find_row_by_id("camera"), Some(2));
        grid.scroll_up(1);
        assert_eq!(grid.find_row_by_id("camera"), Some(1));
        grid.scroll_up(2);
        assert_eq!(grid.find_row_by_id("camera"), None);
    }
}
