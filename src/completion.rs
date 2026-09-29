//! Finding the identifier to complete and the candidates for it.

use std::collections::HashMap;

use regex::Regex;

/// Characters trimmed from the end of a candidate to offer a nested variant,
/// e.g. `127.0.0.1` for `127.0.0.1)`.
const TRIM_RIGHT: &[char] = &[')', ']', '"', '\'', ':'];

/// A piece of text at a position in a pane. Columns are character indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub pane: usize,
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

    fn position(&self) -> (usize, usize, usize) {
        (self.pane, self.line, self.column)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub word: Word,
    /// For nested candidates, the full match they were trimmed from.
    pub parent: Option<String>,
}

/// Finds the identifier that ends at `column` of `text` using `pattern`.
pub fn find_identifier(
    pattern: &Regex,
    text: &str,
    pane: usize,
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
            pane,
            line,
            column,
            value: value.to_owned(),
        })
    }))
}

/// Builds the regexp for candidates: `pattern` preceded by the identifier,
/// if there is one.
pub fn candidate_matcher(
    pattern: &Regex,
    identifier: Option<&Word>,
) -> Result<Regex, regex::Error> {
    match identifier {
        Some(identifier) => Regex::new(&format!(
            "{}(?:{})",
            regex::escape(&identifier.value),
            pattern.as_str()
        )),
        None => Ok(pattern.clone()),
    }
}

/// Appends the candidates found by `matcher` in the lines of `pane`. With an
/// identifier, candidates must start with it and the identifier itself is
/// excluded.
pub fn find_candidates<S: AsRef<str>>(
    matcher: &Regex,
    pane: usize,
    lines: &[S],
    identifier: Option<&Word>,
    candidates: &mut Vec<Candidate>,
) {
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
                    let at_identifier = identifier.position() == (pane, line_number, column);
                    if !value.starts_with(&identifier.value)
                        || value == identifier.value
                        || at_identifier
                    {
                        continue;
                    }
                }

                candidates.push(Candidate {
                    word: Word {
                        pane,
                        line: line_number,
                        column,
                        value: value.to_owned(),
                    },
                    parent,
                });
            }
        }
    }
}

/// Removes duplicates (same value and parent), keeping the occurrence with
/// the highest `priority`, or the last one among equals. The result is
/// ordered by position.
pub fn dedup<K: Ord>(
    candidates: Vec<Candidate>,
    priority: impl Fn(&Candidate) -> K,
) -> Vec<Candidate> {
    let mut best: HashMap<(String, Option<String>), usize> = HashMap::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let key = (candidate.word.value.clone(), candidate.parent.clone());
        best.entry(key)
            .and_modify(|kept| {
                if priority(candidate) >= priority(&candidates[*kept]) {
                    *kept = index;
                }
            })
            .or_insert(index);
    }

    let mut keep: Vec<usize> = best.into_values().collect();
    // Candidates are found in position order, full matches before the
    // nested ones trimmed from them, so index order is position order.
    keep.sort_unstable();
    let mut candidates: Vec<Option<Candidate>> = candidates.into_iter().map(Some).collect();
    keep.into_iter()
        .map(|index| candidates[index].take().expect("each index once"))
        .collect()
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

    fn find(pattern: &str, lines: &[&str]) -> Vec<Candidate> {
        let mut found = Vec::new();
        find_candidates(&regex(pattern), 0, lines, None, &mut found);
        found
    }

    #[test]
    fn identifier_uses_character_columns() {
        let id = find_identifier(&regex("[!-~]+"), "∞ ∞ foo bar", 0, 1, 7)
            .unwrap()
            .unwrap();
        assert_eq!((id.column, id.value.as_str()), (4, "foo"));
    }

    #[test]
    fn no_identifier_after_space() {
        assert_eq!(
            find_identifier(&regex("[!-~]+"), "$ ", 0, 0, 2).unwrap(),
            None
        );
    }

    #[test]
    fn identifier_is_excluded_only_in_its_pane() {
        let id = find_identifier(&regex("[!-~]+"), "foo", 0, 0, 3)
            .unwrap()
            .unwrap();
        let matcher = candidate_matcher(&regex("[a-z]*"), Some(&id)).unwrap();
        let mut found = Vec::new();
        find_candidates(&matcher, 0, &["foo foobar"], Some(&id), &mut found);
        find_candidates(&matcher, 1, &["foobaz"], Some(&id), &mut found);
        assert_eq!(values(&found), ["foobar", "foobaz"]);
    }

    #[test]
    fn nested_candidates() {
        let found = find("[0-9.)]+", &["(127.0.0.1)"]);
        assert_eq!(values(&found), ["127.0.0.1)", "127.0.0.1"]);
        assert_eq!(found[1].parent.as_deref(), Some("127.0.0.1)"));
    }

    #[test]
    fn dedup_keeps_last_occurrence_among_equals() {
        let found = dedup(find("[a-z]+", &["foo bar", "baz foo"]), |_| 0);
        assert_eq!(values(&found), ["bar", "baz", "foo"]);
        assert_eq!(found[2].word.line, 1);
    }

    #[test]
    fn dedup_prefers_priority() {
        let found = dedup(find("[a-z]+", &["foo bar", "baz foo"]), |c| {
            c.word.line == 0
        });
        assert_eq!(values(&found), ["foo", "bar", "baz"]);
        assert_eq!(found[0].word.line, 0);
    }
}
