//! State of the interactive picker: selection, movement, filtering and
//! scrolling. Drawing is left to `ui`.

use std::cell::RefCell;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::completion::{Candidate, Word};
use crate::layout::Cell;
use crate::window::PaneView;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Keys move the selection.
    Normal,
    /// Keys edit the filter.
    Filter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Backspace,
    Tab,
    BackTab,
    /// Ctrl-C.
    Interrupt,
}

/// What the user decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Continue,
    /// Insert the candidate with this index.
    Insert(usize),
    /// Copy the candidate with this index.
    Copy(usize),
    Cancel,
}

pub struct Picker {
    pub panes: Vec<PaneView>,
    pub active: usize,
    pub identifier: Option<Word>,
    pub candidates: Vec<Candidate>,
    /// Row and column of each candidate in its pane.
    cells: Vec<Cell>,
    /// Where the cursor was in the active pane (the start of the identifier
    /// when there is one).
    anchor: Cell,
    pub mode: Mode,
    pub filter: String,
    pattern: Pattern,
    /// Reused between searches; in a cell so drawing can ask for the
    /// matched characters.
    matcher: RefCell<Matcher>,
    /// Indices of candidates matching the filter: in position order without
    /// a filter, best match first with one.
    matches: Vec<usize>,
    selected: Option<usize>,
}

impl Picker {
    pub fn new(
        panes: Vec<PaneView>,
        active: usize,
        identifier: Option<Word>,
        candidates: Vec<Candidate>,
        cursor: (usize, usize),
    ) -> Self {
        let cells = candidates
            .iter()
            .map(|c| panes[c.word.pane].cell(c.word.line, c.word.column))
            .collect();
        let (line, column) = identifier
            .as_ref()
            .map_or(cursor, |id| (id.line, id.column));
        let anchor = panes[active].cell(line, column);
        let mut picker = Self {
            panes,
            active,
            identifier,
            matches: (0..candidates.len()).collect(),
            candidates,
            cells,
            anchor,
            mode: Mode::Normal,
            filter: String::new(),
            pattern: Pattern::default(),
            matcher: RefCell::new(Matcher::new(Config::DEFAULT)),
            selected: None,
        };
        picker.select(picker.best_match());
        picker
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Indices of the candidates currently offered: in position order
    /// without a filter, best match first with one.
    pub fn matches(&self) -> &[usize] {
        &self.matches
    }

    /// Character indices of `candidate` matched by the filter, sorted.
    pub fn matched_chars(&self, candidate: usize) -> Vec<usize> {
        if self.filter.is_empty() {
            return Vec::new();
        }
        let mut buffer = Vec::new();
        let mut indices = Vec::new();
        let haystack = Utf32Str::new(&self.candidates[candidate].word.value, &mut buffer);
        self.pattern
            .indices(haystack, &mut self.matcher.borrow_mut(), &mut indices);
        indices.sort_unstable();
        indices.dedup();
        indices.into_iter().map(|i| i as usize).collect()
    }

    pub fn handle(&mut self, key: Key) -> Outcome {
        match self.mode {
            Mode::Normal => self.handle_normal(key),
            Mode::Filter => self.handle_filter(key),
        }
    }

    fn handle_normal(&mut self, key: Key) -> Outcome {
        match key {
            Key::Up | Key::Char('k') => self.go(Direction::Up),
            Key::Down | Key::Char('j') => self.go(Direction::Down),
            Key::Left | Key::Char('h') => self.go(Direction::Left),
            Key::Right | Key::Char('l') => self.go(Direction::Right),
            Key::Char('n') | Key::Tab => self.cycle(true),
            Key::Char('N') | Key::BackTab => self.cycle(false),
            Key::Char('/') => {
                self.mode = Mode::Filter;
                self.set_filter(String::new());
            }
            Key::Enter => return self.selected.map_or(Outcome::Continue, Outcome::Insert),
            Key::Char('y') => return self.selected.map_or(Outcome::Continue, Outcome::Copy),
            Key::Escape | Key::Interrupt | Key::Char('q') => return Outcome::Cancel,
            _ => {}
        }
        Outcome::Continue
    }

    fn handle_filter(&mut self, key: Key) -> Outcome {
        match key {
            Key::Char(ch) => {
                let mut filter = self.filter.clone();
                filter.push(ch);
                self.set_filter(filter);
            }
            Key::Backspace => {
                let mut filter = self.filter.clone();
                if filter.pop().is_none() {
                    self.mode = Mode::Normal;
                }
                self.set_filter(filter);
            }
            Key::Up => self.go(Direction::Up),
            Key::Down => self.go(Direction::Down),
            Key::Left => self.go(Direction::Left),
            Key::Right => self.go(Direction::Right),
            Key::Tab => self.cycle(true),
            Key::BackTab => self.cycle(false),
            Key::Enter => return self.selected.map_or(Outcome::Continue, Outcome::Insert),
            Key::Escape => self.mode = Mode::Normal,
            Key::Interrupt => return Outcome::Cancel,
        }
        Outcome::Continue
    }

    /// Applies a fuzzy filter (fzf syntax, smart case) and selects the best
    /// match; ties go to the candidate nearest the cursor.
    fn set_filter(&mut self, filter: String) {
        self.pattern = Pattern::parse(&filter, CaseMatching::Smart, Normalization::Smart);
        self.filter = filter;

        if self.filter.is_empty() {
            self.matches = (0..self.candidates.len()).collect();
            if self.selected.is_none() {
                self.select(self.best_match());
            }
            return;
        }

        let mut scored: Vec<(u32, usize)> = {
            let mut matcher = self.matcher.borrow_mut();
            let mut buffer = Vec::new();
            (0..self.candidates.len())
                .filter_map(|index| {
                    let haystack = Utf32Str::new(&self.candidates[index].word.value, &mut buffer);
                    self.pattern
                        .score(haystack, &mut matcher)
                        .map(|score| (score, index))
                })
                .collect()
        };
        scored.sort_by_cached_key(|&(score, index)| {
            std::cmp::Reverse((score, self.proximity(index)))
        });
        self.matches = scored.into_iter().map(|(_, index)| index).collect();
        self.select(self.matches.first().copied());
    }

    fn select(&mut self, index: Option<usize>) {
        self.selected = index;
        if let Some(index) = index {
            let pane = self.candidates[index].word.pane;
            self.panes[pane].reveal(self.cells[index].row);
        }
    }

    fn go(&mut self, direction: Direction) {
        if let Some(next) = self.selected.and_then(|s| self.neighbour(s, direction)) {
            self.select(Some(next));
        }
    }

    /// Moves to the next or previous match (see `matches`), wrapping around.
    fn cycle(&mut self, forward: bool) {
        let Some(selected) = self.selected else {
            return;
        };
        let count = self.matches.len();
        let position = self.position_of(selected).unwrap_or(0);
        let next = if forward {
            (position + 1) % count
        } else {
            (position + count - 1) % count
        };
        self.select(Some(self.matches[next]));
    }

    /// Position of `candidate` in `matches`.
    pub fn position_of(&self, candidate: usize) -> Option<usize> {
        self.matches.iter().position(|&index| index == candidate)
    }

    /// The candidate to select without a filter: the nearest one before the
    /// cursor in the active pane, otherwise one in view.
    fn best_match(&self) -> Option<usize> {
        self.matches
            .iter()
            .copied()
            .max_by_key(|&index| self.proximity(index))
    }

    /// How close a candidate is to where the user is working, higher being
    /// closer: before the cursor in the active pane (nearest first), then in
    /// view, in the active pane, top-level rather than nested, and recent.
    fn proximity(&self, index: usize) -> (bool, bool, bool, bool, Cell) {
        let candidate = &self.candidates[index];
        let pane = candidate.word.pane;
        let cell = self.cells[index];
        let top_level = candidate.parent.is_none();
        let active = pane == self.active;
        (
            active && top_level && cell <= self.anchor,
            self.panes[pane].is_in_view(cell.row),
            active,
            top_level,
            cell,
        )
    }

    /// The match to move to from `from` in `direction`.
    ///
    /// Movement stays within the pane first: horizontally along the row,
    /// ordered by column and then length (so nested candidates come before
    /// the longer ones they were trimmed from), vertically to the nearest
    /// row, scrolling through history. At the edge of a pane it continues in
    /// the nearest pane in that direction.
    fn neighbour(&self, from: usize, direction: Direction) -> Option<usize> {
        let pane = self.candidates[from].word.pane;
        let here = self.cells[from];
        let key = |index: usize| (self.cells[index].col, self.candidates[index].word.len());
        let same_pane = || {
            self.matches
                .iter()
                .copied()
                .filter(move |&index| index != from && self.candidates[index].word.pane == pane)
        };

        let within = match direction {
            Direction::Right => same_pane()
                .filter(|&i| self.cells[i].row == here.row && key(i) > key(from))
                .min_by_key(|&i| key(i)),
            Direction::Left => same_pane()
                .filter(|&i| self.cells[i].row == here.row && key(i) < key(from))
                .max_by_key(|&i| key(i)),
            Direction::Up | Direction::Down => same_pane()
                .filter(|&i| {
                    let row = self.cells[i].row;
                    if direction == Direction::Down {
                        row > here.row
                    } else {
                        row < here.row
                    }
                })
                .min_by_key(|&i| {
                    (
                        self.cells[i].row.abs_diff(here.row),
                        self.cells[i].col.abs_diff(here.col),
                    )
                }),
        };
        within.or_else(|| self.neighbour_in_other_pane(from, direction))
    }

    fn neighbour_in_other_pane(&self, from: usize, direction: Direction) -> Option<usize> {
        let rect = self.panes[self.candidates[from].word.pane].rect;
        let (from_row, from_col) = self.window_position(from);

        self.matches
            .iter()
            .copied()
            .filter(|&index| {
                let other = self.panes[self.candidates[index].word.pane].rect;
                match direction {
                    Direction::Up => other.bottom() <= rect.y,
                    Direction::Down => other.y >= rect.bottom(),
                    Direction::Left => other.right() <= rect.x,
                    Direction::Right => other.x >= rect.right(),
                }
            })
            .min_by_key(|&index| {
                let pane = &self.panes[self.candidates[index].word.pane];
                let (row, col) = self.window_position(index);
                (
                    !pane.is_in_view(self.cells[index].row),
                    row.abs_diff(from_row),
                    col.abs_diff(from_col),
                )
            })
    }

    /// Window row and column of a candidate, even when it is out of view.
    fn window_position(&self, index: usize) -> (isize, usize) {
        let pane = &self.panes[self.candidates[index].word.pane];
        let cell = self.cells[index];
        (pane.window_row(cell.row), pane.rect.x + cell.col)
    }
}

#[cfg(test)]
mod tests {
    use regex::Regex;

    use super::*;
    use crate::completion::{dedup, find_candidates};
    use crate::window::Rect;
    use crate::window::tests::view;

    /// Two panes side by side, the active one on the left with history.
    ///
    /// ```text
    /// left (active)            right
    /// [history] foo_old        ....
    /// [history] foo_older
    /// foo_a foo_b              foo_r1
    /// foo_c                    foo_r2
    /// $ foo                    foo_r3
    /// ```
    fn picker() -> Picker {
        let left = Rect {
            x: 0,
            y: 0,
            width: 20,
            height: 3,
        };
        let right = Rect {
            x: 21,
            y: 0,
            width: 20,
            height: 3,
        };
        let panes = vec![
            view(
                left,
                &["foo_old", "foo_older"],
                &["foo_a foo_b", "foo_c", "$ foo"],
            ),
            view(right, &[], &["foo_r1", "foo_r2", "foo_r3"]),
        ];
        let identifier = Word {
            pane: 0,
            line: 4,
            column: 2,
            value: "foo".into(),
        };
        let matcher = Regex::new("foo[a-z0-9_]*").unwrap();
        let mut candidates = Vec::new();
        for (index, pane) in panes.iter().enumerate() {
            find_candidates(
                &matcher,
                index,
                &pane.lines,
                Some(&identifier),
                &mut candidates,
            );
        }
        let candidates = dedup(candidates, |_| 0);
        Picker::new(panes, 0, Some(identifier), candidates, (4, 5))
    }

    fn selected(picker: &Picker) -> &str {
        &picker.candidates[picker.selected().unwrap()].word.value
    }

    fn press(picker: &mut Picker, keys: &str) {
        for ch in keys.chars() {
            picker.handle(Key::Char(ch));
        }
    }

    #[test]
    fn starts_before_cursor() {
        assert_eq!(selected(&picker()), "foo_c");
    }

    #[test]
    fn moves_within_pane_then_across() {
        let mut p = picker();
        press(&mut p, "k");
        assert_eq!(selected(&p), "foo_a");
        press(&mut p, "l");
        assert_eq!(selected(&p), "foo_b");
        press(&mut p, "l");
        assert_eq!(selected(&p), "foo_r1", "continues in the pane to the right");
        press(&mut p, "j");
        assert_eq!(selected(&p), "foo_r2");
        press(&mut p, "h");
        assert_eq!(selected(&p), "foo_c", "nearest row in the pane to the left");
    }

    #[test]
    fn scrolls_into_history() {
        let mut p = picker();
        assert_eq!(p.panes[0].scrolled_back(), 0);
        press(&mut p, "kk");
        assert_eq!(selected(&p), "foo_older");
        assert_eq!(p.panes[0].scrolled_back(), 1);
        press(&mut p, "k");
        assert_eq!(selected(&p), "foo_old");
        assert_eq!(p.panes[0].scrolled_back(), 2);
        press(&mut p, "jjj");
        assert_eq!(selected(&p), "foo_c");
    }

    #[test]
    fn filters_with_smart_case() {
        let mut p = picker();
        press(&mut p, "/R2");
        assert!(p.matches().is_empty());
        assert_eq!(p.selected(), None);
        assert_eq!(p.handle(Key::Enter), Outcome::Continue);

        p.handle(Key::Backspace);
        p.handle(Key::Backspace);
        press(&mut p, "old");
        assert_eq!(p.matches().len(), 2);
        assert_eq!(selected(&p), "foo_older", "most recent match");
        assert_eq!(p.panes[0].scrolled_back(), 1);

        p.handle(Key::Escape);
        assert_eq!(p.mode, Mode::Normal);
        press(&mut p, "n");
        assert_eq!(selected(&p), "foo_old", "wraps around");
        assert!(matches!(p.handle(Key::Char('y')), Outcome::Copy(_)));
    }

    #[test]
    fn filter_is_fuzzy() {
        let mut p = picker();
        press(&mut p, "/fr3");
        assert_eq!(p.matches().len(), 1);
        assert_eq!(selected(&p), "foo_r3");
        let index = p.selected().unwrap();
        assert_eq!(p.matched_chars(index), [0, 4, 5]);

        // fzf syntax: an exact match.
        p.handle(Key::Escape);
        press(&mut p, "/'o_r");
        assert_eq!(p.matches().len(), 3);
    }

    #[test]
    fn ties_go_to_the_nearest_candidate() {
        let mut p = picker();
        press(&mut p, "/foo");
        assert_eq!(
            selected(&p),
            "foo_c",
            "all score the same; nearest before the cursor wins"
        );
    }

    #[test]
    fn backspace_on_empty_filter_leaves_filter_mode() {
        let mut p = picker();
        press(&mut p, "/r");
        p.handle(Key::Backspace);
        assert_eq!(p.mode, Mode::Filter);
        assert_eq!(p.matches().len(), p.candidates.len());
        p.handle(Key::Backspace);
        assert_eq!(p.mode, Mode::Normal);
    }

    #[test]
    fn jk_are_text_in_filter_mode() {
        let mut p = picker();
        press(&mut p, "/k");
        assert_eq!(p.filter, "k");
        assert_eq!(p.handle(Key::Char('q')), Outcome::Continue);
        assert_eq!(p.handle(Key::Enter), Outcome::Continue, "no match for 'kq'");
    }
}
