//! Finding the identifier to complete, the candidates for it, and moving the
//! selection between candidates.

use std::collections::HashSet;

use regex::Regex;

/// Characters trimmed from the end of a candidate to offer a nested variant,
/// e.g. `127.0.0.1` for `127.0.0.1)`.
const TRIM_RIGHT: &[char] = &[')', ']', '"', '\'', ':'];

/// A piece of text at a position in the pane. Columns are character indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub line: usize,
    pub column: usize,
    pub value: String,
}

impl Word {
    pub fn len(&self) -> usize {
        self.value.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub word: Word,
    /// For nested candidates, the full match they were trimmed from.
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Finds the identifier that ends at `column` of `text` using `pattern`.
pub fn find_identifier(
    pattern: &Regex,
    text: &str,
    line: usize,
    column: usize,
) -> Result<Option<Word>, regex::Error> {
    let before_cursor: String = text.chars().take(column).collect();
    // The lazy prefix makes the identifier start as early as possible.
    let anchored = Regex::new(&format!("^.*?({})$", pattern.as_str()))?;

    Ok(anchored.captures(&before_cursor).and_then(|captures| {
        let value = captures.get(1)?.as_str();
        let column = before_cursor.chars().count() - value.chars().count();
        Some(Word {
            line,
            column,
            value: value.to_owned(),
        })
    }))
}

/// Finds all candidates in `lines`. With an identifier, candidates must start
/// with it and the identifier itself is excluded.
pub fn find_candidates<S: AsRef<str>>(
    pattern: &Regex,
    lines: &[S],
    identifier: Option<&Word>,
) -> Result<Vec<Candidate>, regex::Error> {
    let matcher = match identifier {
        Some(identifier) => Regex::new(&format!(
            "{}(?:{})",
            regex::escape(&identifier.value),
            pattern.as_str()
        ))?,
        None => pattern.clone(),
    };

    let mut candidates = Vec::new();

    for (line_number, line) in lines.iter().enumerate() {
        let line = line.as_ref();

        for found in matcher.find_iter(line) {
            let text = found.as_str();
            let column = line[..found.start()].chars().count();

            let trimmed = text.trim_end_matches(TRIM_RIGHT);
            let nested = (!trimmed.is_empty() && trimmed != text).then_some(trimmed);

            let units = std::iter::once((text, None))
                .chain(nested.map(|value| (value, Some(text.to_owned()))));

            for (value, parent) in units {
                if let Some(identifier) = identifier {
                    let at_identifier =
                        line_number == identifier.line && column == identifier.column;
                    if !value.starts_with(&identifier.value)
                        || value == identifier.value
                        || at_identifier
                    {
                        continue;
                    }
                }

                candidates.push(Candidate {
                    word: Word {
                        line: line_number,
                        column,
                        value: value.to_owned(),
                    },
                    parent,
                });
            }
        }
    }

    Ok(candidates)
}

/// Removes duplicates, keeping the occurrence closest to the end of the pane,
/// which is usually closest to the cursor.
pub fn dedup(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut seen = HashSet::new();
    let mut unique: Vec<Candidate> = candidates
        .into_iter()
        .rev()
        .filter(|c| seen.insert((c.word.value.clone(), c.parent.clone())))
        .collect();
    unique.reverse();
    unique
}

/// Index of the candidate to select initially: the last top-level candidate
/// at or before `anchor`, or the first candidate if there is none.
pub fn default_selection(candidates: &[Candidate], anchor: &Word) -> usize {
    let anchor = (anchor.line, anchor.column);
    candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| c.parent.is_none() && (c.word.line, c.word.column) <= anchor)
        .max_by_key(|(_, c)| (c.word.line, c.word.column))
        .map_or(0, |(index, _)| index)
}

/// Index of the candidate to move to from `selected`, if any.
///
/// Horizontal moves stay on the same line and visit candidates ordered by
/// column, then length, so candidates starting at the same column (nested
/// ones) go from shortest to longest. Vertical moves go to the nearest line
/// in that direction, preferring the closest column.
pub fn navigate(candidates: &[Candidate], selected: usize, direction: Direction) -> Option<usize> {
    let current = &candidates[selected].word;
    let key = |w: &Word| (w.column, w.len());
    let others = candidates
        .iter()
        .enumerate()
        .filter(|&(index, _)| index != selected)
        .map(|(index, c)| (index, &c.word));

    match direction {
        Direction::Left | Direction::Right => {
            let same_line = others.filter(|(_, w)| w.line == current.line);
            if direction == Direction::Right {
                same_line
                    .filter(|(_, w)| key(w) > key(current))
                    .min_by_key(|(_, w)| key(w))
            } else {
                same_line
                    .filter(|(_, w)| key(w) < key(current))
                    .max_by_key(|(_, w)| key(w))
            }
        }
        Direction::Up | Direction::Down => others
            .filter(|(_, w)| {
                if direction == Direction::Down {
                    w.line > current.line
                } else {
                    w.line < current.line
                }
            })
            .min_by_key(|(_, w)| {
                (
                    w.line.abs_diff(current.line),
                    w.column.abs_diff(current.column),
                )
            }),
    }
    .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regex(pattern: &str) -> Regex {
        Regex::new(pattern).unwrap()
    }

    fn values(candidates: &[Candidate]) -> Vec<&str> {
        candidates.iter().map(|c| c.word.value.as_str()).collect()
    }

    #[test]
    fn identifier_uses_character_columns() {
        let id = find_identifier(&regex("[!-~]+"), "∞ ∞ foo bar", 1, 7)
            .unwrap()
            .unwrap();
        assert_eq!((id.column, id.value.as_str()), (4, "foo"));
    }

    #[test]
    fn no_identifier_after_space() {
        assert_eq!(find_identifier(&regex("[!-~]+"), "$ ", 0, 2).unwrap(), None);
    }

    #[test]
    fn nested_candidates() {
        let lines = ["(127.0.0.1)"];
        let found = find_candidates(&regex("[0-9.)]+"), &lines, None).unwrap();
        assert_eq!(values(&found), ["127.0.0.1)", "127.0.0.1"]);
        assert_eq!(found[1].parent.as_deref(), Some("127.0.0.1)"));
    }

    #[test]
    fn dedup_keeps_last_occurrence() {
        let lines = ["foo bar", "baz foo"];
        let found = dedup(find_candidates(&regex("[a-z]+"), &lines, None).unwrap());
        assert_eq!(values(&found), ["bar", "baz", "foo"]);
        assert_eq!(found[2].word.line, 1);
    }

    fn grid() -> Vec<Candidate> {
        // line 0: aa  bb  cc
        // line 1: dd  ee
        // line 2: ff) (nested "ff" at the same column)
        let lines = ["aa  bb  cc", "dd  ee", "ff)"];
        find_candidates(&regex("[a-z)]+"), &lines, None).unwrap()
    }

    #[test]
    fn navigation() {
        let c = grid();
        let at = |value: &str| c.iter().position(|c| c.word.value == value).unwrap();
        let go = |from: &str, d| navigate(&c, at(from), d).map(|i| c[i].word.value.as_str());

        assert_eq!(go("aa", Direction::Right), Some("bb"));
        assert_eq!(go("cc", Direction::Right), None);
        assert_eq!(go("cc", Direction::Left), Some("bb"));
        assert_eq!(go("cc", Direction::Down), Some("ee"));
        assert_eq!(go("ee", Direction::Up), Some("bb"));
        assert_eq!(go("aa", Direction::Up), None);
        assert_eq!(go("ff", Direction::Right), Some("ff)"));
        assert_eq!(go("ff)", Direction::Left), Some("ff"));
    }

    #[test]
    fn default_selection_is_last_before_anchor() {
        let c = grid();
        let anchor = Word {
            line: 1,
            column: 3,
            value: String::new(),
        };
        assert_eq!(c[default_selection(&c, &anchor)].word.value, "dd");
    }
}
