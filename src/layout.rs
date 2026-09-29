//! Mapping between positions in logical lines and cells on screen, taking
//! line wrapping and double-width characters into account.

use unicode_width::UnicodeWidthChar;

/// A cell on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug)]
struct LineLayout {
    first_row: usize,
    /// Row count occupied by the line (at least one).
    rows: usize,
    /// Cell of each character, relative to `first_row`.
    cells: Vec<Cell>,
    /// Cell just after the last character, relative to `first_row`.
    end: Cell,
}

/// How a set of lines is laid out on a screen of a given width.
#[derive(Debug)]
pub struct Layout {
    lines: Vec<LineLayout>,
}

impl Layout {
    pub fn new<S: AsRef<str>>(lines: &[S], width: usize) -> Self {
        let width = width.max(1);
        let mut first_row = 0;
        let lines = lines
            .iter()
            .map(|line| {
                let layout = layout_line(line.as_ref(), width, first_row);
                first_row += layout.rows;
                layout
            })
            .collect();
        Self { lines }
    }

    /// Screen cell of character `char_index` of `line`. An index one past the
    /// end of the line yields the cell after its last character.
    pub fn cell(&self, line: usize, char_index: usize) -> Cell {
        let layout = &self.lines[line];
        let relative = layout.cells.get(char_index).copied().unwrap_or(layout.end);
        Cell {
            row: layout.first_row + relative.row,
            col: relative.col,
        }
    }

    /// Finds the line and character index for a screen cell. The index is
    /// the number of characters of the line placed before `cell`, so a cell
    /// after the end of the text maps to the end of the line.
    pub fn locate(&self, cell: Cell) -> Option<(usize, usize)> {
        let index = self
            .lines
            .iter()
            .position(|l| (l.first_row..l.first_row + l.rows).contains(&cell.row))?;
        let layout = &self.lines[index];
        let relative = Cell {
            row: cell.row - layout.first_row,
            col: cell.col,
        };
        let char_index = layout.cells.partition_point(|&c| c < relative);
        Some((index, char_index))
    }
}

fn layout_line(text: &str, width: usize, first_row: usize) -> LineLayout {
    let mut cells = Vec::with_capacity(text.len());
    let mut cursor = Cell { row: 0, col: 0 };

    for ch in text.chars() {
        let char_width = ch.width().unwrap_or(0);
        // A character that doesn't fit wraps to the next row, as terminals do.
        if cursor.col > 0 && cursor.col + char_width > width {
            cursor = Cell {
                row: cursor.row + 1,
                col: 0,
            };
        }
        cells.push(cursor);
        cursor.col += char_width;
    }

    LineLayout {
        first_row,
        rows: cells.last().map_or(1, |c| c.row + 1),
        cells,
        end: cursor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(row: usize, col: usize) -> Cell {
        Cell { row, col }
    }

    #[test]
    fn wraps_long_lines() {
        let layout = Layout::new(&["abcdefg", "", "xy"], 3);
        assert_eq!(layout.cell(0, 0), cell(0, 0));
        assert_eq!(layout.cell(0, 4), cell(1, 1));
        assert_eq!(layout.cell(0, 6), cell(2, 0));
        assert_eq!(layout.cell(1, 0), cell(3, 0));
        assert_eq!(layout.cell(2, 1), cell(4, 1));
        assert_eq!(layout.cell(2, 2), cell(4, 2));
    }

    #[test]
    fn handles_wide_characters() {
        // "日本" occupies four columns; the second one wraps on a width of 3.
        let layout = Layout::new(&["a日本b"], 3);
        assert_eq!(layout.cell(0, 1), cell(0, 1));
        assert_eq!(layout.cell(0, 2), cell(1, 0));
        assert_eq!(layout.cell(0, 3), cell(1, 2));
    }

    #[test]
    fn locates_cells() {
        let layout = Layout::new(&["abcdefg", "∞ ab"], 3);
        assert_eq!(layout.locate(cell(1, 1)), Some((0, 4)));
        assert_eq!(layout.locate(cell(2, 1)), Some((0, 7)));
        assert_eq!(layout.locate(cell(3, 3)), Some((1, 3)));
        assert_eq!(layout.locate(cell(4, 1)), Some((1, 4)));
        assert_eq!(layout.locate(cell(5, 0)), None);
    }
}
