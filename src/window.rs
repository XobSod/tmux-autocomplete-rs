//! The panes of a tmux window, with their contents and scrollback.

use anyhow::{Context, Result, bail};

use crate::layout::{Cell, Layout};
use crate::pane::{self, Line};
use crate::tmux;

/// A rectangle of cells in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    pub fn right(&self) -> usize {
        self.x + self.width
    }

    pub fn bottom(&self) -> usize {
        self.y + self.height
    }

    pub fn contains(&self, cell: Cell) -> bool {
        (self.x..self.right()).contains(&cell.col) && (self.y..self.bottom()).contains(&cell.row)
    }
}

/// A pane with its scrollback and live screen, laid out at the pane width.
/// Rows are counted from the oldest line of scrollback.
#[derive(Debug)]
pub struct PaneView {
    pub id: String,
    pub rect: Rect,
    pub lines: Vec<Line>,
    pub layout: Layout,
    /// First row of the live screen; rows above it are scrollback.
    pub screen_top: usize,
    /// Row shown at the top of the pane.
    pub scroll: usize,
}

impl PaneView {
    pub fn new(id: String, rect: Rect, history: Vec<Line>, screen: Vec<Line>) -> Self {
        let history_lines = history.len();
        let mut lines = history;
        lines.extend(screen);
        let layout = Layout::new(&lines, rect.width);
        let screen_top = if history_lines < lines.len() {
            layout.first_row(history_lines)
        } else {
            layout.rows()
        };
        Self {
            id,
            rect,
            lines,
            layout,
            screen_top,
            scroll: screen_top,
        }
    }

    /// Row and column of character `char_index` of `line`.
    pub fn cell(&self, line: usize, char_index: usize) -> Cell {
        self.layout.cell(line, &self.lines[line].text, char_index)
    }

    /// Line and character index at `cell` of the live screen.
    pub fn locate_on_screen(&self, cell: Cell) -> Option<(usize, usize)> {
        let cell = Cell {
            row: self.screen_top + cell.row,
            col: cell.col,
        };
        self.layout.locate(&self.lines, cell)
    }

    pub fn is_in_view(&self, row: usize) -> bool {
        (self.scroll..self.scroll + self.rect.height).contains(&row)
    }

    /// Window position of `cell`, if it is in view.
    pub fn to_window(&self, cell: Cell) -> Option<Cell> {
        self.is_in_view(cell.row).then(|| Cell {
            row: self.rect.y + cell.row - self.scroll,
            col: self.rect.x + cell.col,
        })
    }

    /// Window row `row` would be drawn at, even when it is out of view.
    pub fn window_row(&self, row: usize) -> isize {
        as_isize(self.rect.y + row) - as_isize(self.scroll)
    }

    /// Scrolls the least needed to bring `row` into view.
    pub fn reveal(&mut self, row: usize) {
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll + self.rect.height {
            self.scroll = row + 1 - self.rect.height;
        }
        self.scroll = self.scroll.min(self.screen_top);
    }

    /// How many rows the pane is scrolled back from its live screen.
    pub fn scrolled_back(&self) -> usize {
        self.screen_top.saturating_sub(self.scroll)
    }
}

fn as_isize(value: usize) -> isize {
    isize::try_from(value).unwrap_or(isize::MAX)
}

/// How much scrollback to read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum History {
    All,
    Lines(usize),
}

/// Captures the panes of the window containing `active`. Returns them in
/// tmux order and the index of `active`. A zoomed window only has the zoomed
/// pane.
pub fn capture(active: &str, history: History) -> Result<(Vec<PaneView>, usize)> {
    let listing = tmux::list_panes(
        active,
        "#{pane_id}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}\t\
         #{history_size}\t#{window_zoomed_flag}",
    )?;

    let mut geometry = Vec::new();
    for entry in listing.lines() {
        let fields: Vec<&str> = entry.split('\t').collect();
        let [id, x, y, width, height, history_size, zoomed] = fields[..] else {
            bail!("unexpected pane listing from tmux: {entry:?}");
        };
        if zoomed == "1" && id != active {
            continue;
        }
        let number = |value: &str| {
            value
                .parse::<usize>()
                .with_context(|| format!("invalid pane geometry in {entry:?}"))
        };
        let mut rect = Rect {
            x: number(x)?,
            y: number(y)?,
            width: number(width)?,
            height: number(height)?,
        };
        if zoomed == "1" {
            rect.x = 0;
            rect.y = 0;
        }
        let history_start = match (history, number(history_size)?) {
            (_, 0) | (History::Lines(0), _) => None,
            (History::All, _) => Some("-".to_owned()),
            (History::Lines(lines), _) => Some(format!("-{lines}")),
        };
        geometry.push((id, rect, history_start));
    }

    let mut panes = Vec::new();
    for (id, rect, history_start) in geometry {
        let history = match history_start {
            Some(start) => pane::parse(&tmux::capture_pane(id, Some((&start, "-1")))?),
            None => Vec::new(),
        };
        let screen = pane::parse(&tmux::capture_pane(id, None)?);
        panes.push(PaneView::new(id.to_owned(), rect, history, screen));
    }

    let active_index = panes
        .iter()
        .position(|pane| pane.id == active)
        .with_context(|| format!("pane {active} not found in its window"))?;
    Ok((panes, active_index))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn lines(texts: &[&str]) -> Vec<Line> {
        texts.iter().flat_map(|text| pane::parse(text)).collect()
    }

    pub fn view(rect: Rect, history: &[&str], screen: &[&str]) -> PaneView {
        PaneView::new("%0".into(), rect, lines(history), lines(screen))
    }

    const RECT: Rect = Rect {
        x: 2,
        y: 1,
        width: 10,
        height: 2,
    };

    #[test]
    fn screen_starts_after_history() {
        let pane = view(RECT, &["old one", "a very long old line"], &["now", "$ x"]);
        assert_eq!(pane.screen_top, 3);
        assert_eq!(pane.scroll, 3);
        assert_eq!(pane.locate_on_screen(Cell { row: 1, col: 3 }), Some((3, 3)));
        assert_eq!(
            pane.to_window(pane.cell(2, 1)),
            Some(Cell { row: 1, col: 3 })
        );
        assert_eq!(pane.to_window(pane.cell(0, 0)), None);
    }

    #[test]
    fn reveal_scrolls_minimally() {
        let mut pane = view(RECT, &["h0", "h1", "h2", "h3"], &["s0", "s1"]);
        pane.reveal(1);
        assert_eq!((pane.scroll, pane.scrolled_back()), (1, 3));
        pane.reveal(3);
        assert_eq!(pane.scroll, 2);
        pane.reveal(5);
        assert_eq!(pane.scroll, 4);
        assert_eq!(pane.window_row(0), -3);
    }
}
