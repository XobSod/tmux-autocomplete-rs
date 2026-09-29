//! The scenarios of the original project's tests, with our own inputs. Each
//! file in `tests/fixtures` names a pane capture from `input/`, the text typed
//! at a `$ ` prompt below it, and the expected candidates in order.

use std::fs;
use std::path::Path;

use regex::Regex;
use tmux_autocomplete::cli::{DEFAULT_REGEXP_CANDIDATE, DEFAULT_REGEXP_CURSOR};
use tmux_autocomplete::completion::{candidate_matcher, dedup, find_candidates, find_identifier};

fn candidates_for(scenario: &Path) -> (Vec<String>, Vec<String>) {
    let spec = fs::read_to_string(scenario).unwrap();
    let mut spec = spec.trim().lines();
    let input = spec.next().expect("input file");
    let prompt = spec.next().expect("prompt");
    let expected = spec.map(str::to_owned).collect();

    let pane = fs::read_to_string(scenario.parent().unwrap().join(input)).unwrap();
    let mut lines: Vec<String> = pane.split('\n').map(str::to_owned).collect();
    let prompt_line = format!("$ {prompt}");
    let cursor = prompt_line.chars().count();
    lines.push(prompt_line);

    let last = lines.len() - 1;
    let identifier = find_identifier(
        &Regex::new(DEFAULT_REGEXP_CURSOR).unwrap(),
        &lines[last],
        0,
        last,
        cursor,
    )
    .unwrap()
    .expect("identifier at the prompt");

    let matcher = candidate_matcher(
        &Regex::new(DEFAULT_REGEXP_CANDIDATE).unwrap(),
        Some(&identifier),
    )
    .unwrap();
    let mut found = Vec::new();
    find_candidates(&matcher, 0, &lines, Some(&identifier), &mut found);
    // As in the original: of duplicates, the last one is kept.
    let found = dedup(found, |_| ());
    (found.into_iter().map(|c| c.word.value).collect(), expected)
}

#[test]
fn scenarios() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut count = 0;
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let (actual, expected) = candidates_for(&path);
            assert_eq!(actual, expected, "scenario {}", path.display());
            count += 1;
        }
    }
    assert!(count >= 5, "only {count} scenarios found");
}
