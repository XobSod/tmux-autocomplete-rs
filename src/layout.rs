//! Mapping between positions in logical lines and cells on screen, taking
//! line wrapping and double-width characters into account.
//!
//! Only the first row of each line is stored, so that long scrollback stays
//! cheap; cells within a line are computed from its text when needed.

use unicode_width::UnicodeWidthChar;

/// A cell on screen, or in the rows of a laid out text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
}

/// How a set of lines wraps at a given width.
#[derive(Debug, Clone)]
pub struct Layout {
    width: usize,
    /// First row of each line, followed by the total row count.
    first_rows: Vec<usize>,
}

impl Layout {
    pub fn new<S: AsRef<str>>(lines: &[S], width: usize) -> Self {
        let width = width.max(1);
        let mut first_rows = Vec::with_capacity(lines.len() + 1);
        let mut row = 0;
        for line in lines {
            first_rows.push(row);
            row += cells(line.as_ref(), width)
                .last()
                .map_or(1, |(_, c)| c.row + 1);
        }
        first_rows.push(row);
        Self { width, first_rows }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn line_count(&self) -> usize {
        self.first_rows.len() - 1
    }

    /// Total number of rows.
    pub fn rows(&self) -> usize {
        *self.first_rows.last().expect("sentinel row")
    }

    pub fn first_row(&self, line: usize) -> usize {
        self.first_rows[line]
    }

    /// The line that `row` belongs to.
    pub fn line_at_row(&self, row: usize) -> Option<usize> {
        if row >= self.rows() {
            return None;
        }
        Some(self.first_rows.partition_point(|&first| first <= row) - 1)
    }

    /// Cells of the characters of `text`, which must be line `line`. The
    /// cell of index `text.chars().count()` is the one after the last
    /// character.
    pub fn cell(&self, line: usize, text: &str, char_index: usize) -> Cell {
        let relative = cells(text, self.width)
            .nth(char_index)
            .map(|(_, cell)| cell)
            .unwrap_or_else(|| end_cell(text, self.width));
        Cell {
            row: self.first_rows[line] + relative.row,
            col: relative.col,
        }
    }

    /// Characters of `text` (line `line`) with their absolute cells.
    pub fn chars<'a>(&self, line: usize, text: &'a str) -> impl Iterator<Item = (char, Cell)> + 'a {
        let first_row = self.first_rows[line];
        cells(text, self.width).map(move |(ch, cell)| {
            (
                ch,
                Cell {
                    row: first_row + cell.row,
                    col: cell.col,
                },
            )
        })
    }

    /// Finds the line and character index for a cell of `lines`. The index
    /// is the number of characters of the line placed before `cell`, so a
    /// cell after the end of the text maps to the end of the line.
    pub fn locate<S: AsRef<str>>(&self, lines: &[S], cell: Cell) -> Option<(usize, usize)> {
        let line = self.line_at_row(cell.row)?;
        let relative = Cell {
            row: cell.row - self.first_rows[line],
            col: cell.col,
        };
        let char_index = cells(lines[line].as_ref(), self.width)
            .take_while(|&(_, c)| c < relative)
            .count();
        Some((line, char_index))
    }
}

/// Characters of `text` with their cells relative to the start of the line.
fn cells(text: &str, width: usize) -> impl Iterator<Item = (char, Cell)> + '_ {
    let mut cursor = Cell { row: 0, col: 0 };
    text.chars().map(move |ch| {
        let char_width = ch.width().unwrap_or(0);
        // A character that doesn't fit wraps to the next row, as terminals do.
        if cursor.col > 0 && cursor.col + char_width > width {
            cursor = Cell {
                row: cursor.row + 1,
                col: 0,
            };
        }
        let cell = cursor;
        cursor.col += char_width;
        (ch, cell)
    })
}

fn end_cell(text: &str, width: usize) -> Cell {
    match text.chars().last() {
        None => Cell { row: 0, col: 0 },
        Some(last) => {
            let (_, cell) = cells(text, width).last().expect("non-empty");
            Cell {
                row: cell.row,
                col: cell.col + last.width().unwrap_or(0),
            }
        }
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
        let lines = ["abcdefg", "", "xy"];
        let layout = Layout::new(&lines, 3);
        let at = |line: usize, index| layout.cell(line, lines[line], index);
        assert_eq!(at(0, 0), cell(0, 0));
        assert_eq!(at(0, 4), cell(1, 1));
        assert_eq!(at(0, 6), cell(2, 0));
        assert_eq!(at(1, 0), cell(3, 0));
        assert_eq!(at(2, 1), cell(4, 1));
        assert_eq!(at(2, 2), cell(4, 2));
        assert_eq!(layout.rows(), 5);
        assert_eq!(layout.line_at_row(2), Some(0));
        assert_eq!(layout.line_at_row(3), Some(1));
        assert_eq!(layout.line_at_row(5), None);
    }

    #[test]
    fn handles_wide_characters() {
        // "日本" occupies four columns; the second one wraps on a width of 3.
        let text = "a日本b";
        let layout = Layout::new(&[text], 3);
        assert_eq!(layout.cell(0, text, 1), cell(0, 1));
        assert_eq!(layout.cell(0, text, 2), cell(1, 0));
        assert_eq!(layout.cell(0, text, 3), cell(1, 2));
        assert_eq!(layout.cell(0, text, 4), cell(1, 3));
    }

    #[test]
    fn locates_cells() {
        let lines = ["abcdefg", "∞ ab"];
        let layout = Layout::new(&lines, 3);
        assert_eq!(layout.locate(&lines, cell(1, 1)), Some((0, 4)));
        assert_eq!(layout.locate(&lines, cell(2, 1)), Some((0, 7)));
        assert_eq!(layout.locate(&lines, cell(3, 3)), Some((1, 3)));
        assert_eq!(layout.locate(&lines, cell(4, 1)), Some((1, 4)));
        assert_eq!(layout.locate(&lines, cell(5, 0)), None);
    }
}
