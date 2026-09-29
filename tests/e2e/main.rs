//! End-to-end tests: real tmux servers, a real client, the real binary.
//!
//! Each test gets its own pair of tmux servers (see `rig`), so they run in
//! parallel and never touch the user's tmux. They need tmux 3.3 or later.

mod rig;

use std::fs;

use rig::{IDENTIFIER, Rig};

const PANE: &str = "%0";

#[test]
fn completes_the_word_before_the_cursor() {
    let rig = Rig::start(80, 10);
    rig.run(PANE, "echo hello_world helium_balloon hexagon");
    rig.type_text(PANE, "echo he");

    let view = rig.open(PANE, &[]);
    // Both the command line and its output have the words; each is offered
    // once, at its latest position.
    assert_eq!(
        view.candidates(),
        ["hello_world", "helium_balloon", "hexagon"]
    );
    assert_eq!(view.selected().as_deref(), Some("hexagon"));
    assert_eq!(view.runs(IDENTIFIER), ["he"]);

    rig.keys(&["Left", "Enter"]);
    rig.wait_for_line(PANE, "$ echo helium_balloon");
}

#[test]
fn single_candidate_is_inserted_without_picker() {
    let rig = Rig::start(80, 10);
    rig.run(PANE, "echo unique_word other");
    rig.type_text(PANE, "echo uni");
    rig.trigger(PANE, &[]);
    rig.wait_for_line(PANE, "$ echo unique_word");
}

#[test]
fn nothing_happens_without_candidates() {
    let rig = Rig::start(80, 10);
    rig.type_text(PANE, "echo zzz");
    assert_eq!(rig.trigger_foreground(PANE, &[]), "");
    assert_eq!(rig.last_line(PANE), "$ echo zzz");
}

#[test]
fn escape_cancels() {
    let rig = Rig::start(80, 10);
    rig.run(PANE, "echo alpha_one alpha_two");
    rig.type_text(PANE, "echo al");
    rig.open(PANE, &[]);
    rig.keys(&["Escape"]);
    rig.wait_for_line(PANE, "$ echo al");
}

#[test]
fn navigates_with_hjkl() {
    let rig = Rig::start(80, 10);
    rig.run(PANE, "echo xa_1 xa_2; echo xa_3");
    rig.type_text(PANE, "echo xa");

    let view = rig.open(PANE, &[]);
    assert_eq!(view.selected().as_deref(), Some("xa_3"));
    rig.keys(&["k"]);
    rig.wait_view("k to select xa_1", |v| {
        v.selected().as_deref() == Some("xa_1")
    });
    rig.keys(&["l"]);
    rig.wait_view("l to select xa_2", |v| {
        v.selected().as_deref() == Some("xa_2")
    });
    rig.keys(&["j", "Enter"]);
    rig.wait_for_line(PANE, "$ echo xa_3");
}

#[test]
fn nested_candidates_are_offered() {
    let rig = Rig::start(80, 10);
    rig.run(PANE, "echo '(10.0.0.1)'");
    rig.type_text(PANE, "echo 10.");

    let view = rig.open(PANE, &[]);
    assert_eq!(view.selected().as_deref(), Some("10.0.0.1)"));
    rig.keys(&["Left", "Enter"]);
    rig.wait_for_line(PANE, "$ echo 10.0.0.1");
}

#[test]
fn finds_candidates_in_other_panes() {
    let rig = Rig::start(100, 10);
    let right = rig.split(PANE, false, None);
    rig.run(&right, "echo right_side_word");
    rig.run(PANE, "echo right_here");
    rig.inner(&["select-pane", "-t", PANE]);
    rig.type_text(PANE, "echo ri");

    let view = rig.open(PANE, &[]);
    assert_eq!(view.selected().as_deref(), Some("right_here"));
    assert!(view.candidates().contains(&"right_side_word".to_owned()));

    rig.keys(&["l", "Enter"]);
    rig.wait_for_line(PANE, "$ echo right_side_word");
}

#[test]
fn zoomed_pane_is_searched_alone() {
    let rig = Rig::start(100, 10);
    let right = rig.split(PANE, false, None);
    rig.run(&right, "echo zoom_hidden");
    rig.run(PANE, "echo zoom_shown");
    rig.inner(&["resize-pane", "-Z", "-t", PANE]);
    rig.type_text(PANE, "echo zoom");

    // The only candidate in the zoomed pane is inserted right away.
    rig.trigger(PANE, &[]);
    rig.wait_for_line(PANE, "$ echo zoom_shown");
}

#[test]
fn scrolls_back_through_history() {
    let rig = Rig::start(80, 8);
    rig.run(PANE, "for i in $(seq 1 30); do echo old_word_$i; done");
    rig.type_text(PANE, "echo old");

    let view = rig.open(PANE, &[]);
    assert_eq!(view.selected().as_deref(), Some("old_word_30"));
    assert!(view.indicators().is_empty());

    for _ in 0..10 {
        rig.keys(&["k"]);
    }
    let view = rig.wait_view("scrolled to old_word_20", |v| {
        v.selected().as_deref() == Some("old_word_20")
    });
    assert_eq!(view.indicators().len(), 1, "scroll indicator shown");

    rig.keys(&["Enter"]);
    rig.wait_for_line(PANE, "$ echo old_word_20");
}

#[test]
fn history_option_limits_scrollback() {
    let rig = Rig::start(80, 8);
    rig.run(
        PANE,
        "echo far_away_word; for i in $(seq 1 30); do echo filler; done; echo far_near",
    );
    rig.type_text(PANE, "echo far");

    // The first word is 32 lines up, out of reach with --history 5.
    rig.trigger(PANE, &["--history", "5"]);
    rig.wait_for_line(PANE, "$ echo far_near");
}

#[test]
fn fuzzy_filter() {
    let rig = Rig::start(80, 8);
    rig.run(PANE, "for i in $(seq 1 30); do echo history_word_$i; done");
    rig.type_text(PANE, "echo hi");

    rig.open(PANE, &[]);
    rig.keys(&["/", "w", "2", "5"]);
    let view = rig.wait_view("filter to select history_word_25", |v| {
        v.selected().as_deref() == Some("history_word_25")
    });
    assert_eq!(view.prompt().as_deref(), Some("/w25  [1/1]"));
    assert_eq!(view.selected_underlined(), "w25");

    rig.keys(&["Enter"]);
    rig.wait_for_line(PANE, "$ echo history_word_25");
}

#[test]
fn filter_takes_j_and_k_as_text() {
    let rig = Rig::start(80, 8);
    rig.run(PANE, "echo kiwi_jam kiwi_tea");
    rig.type_text(PANE, "echo ki");

    rig.open(PANE, &[]);
    rig.keys(&["/", "j", "k"]);
    rig.wait_view("filter /jk", |v| {
        v.prompt().is_some_and(|p| p.starts_with("/jk "))
    });
    rig.keys(&["BSpace", "BSpace", "t"]);
    rig.wait_view("filter /t", |v| v.selected().as_deref() == Some("kiwi_tea"));
    rig.keys(&["Escape", "Enter"]);
    rig.wait_for_line(PANE, "$ echo kiwi_tea");
}

#[test]
fn y_copies_instead_of_inserting() {
    let rig = Rig::start(80, 8);
    rig.run(PANE, "echo copy_me copy_you");
    rig.type_text(PANE, "echo co");

    rig.open(PANE, &[]);
    rig.keys(&["y"]);
    rig.wait_for_line(PANE, "$ echo co");
    assert_eq!(rig.inner(&["show-buffer"]), "copy_you");
}

#[test]
fn exec_runs_program_on_selection() {
    let rig = Rig::start(80, 8);
    let opener = rig.path("opener");
    let opened = rig.path("opened");
    fs::write(
        &opener,
        format!("#!/bin/sh\necho \"$1\" > {}\n", opened.display()),
    )
    .unwrap();
    fs::set_permissions(&opener, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    rig.run(PANE, "echo see https://a.example/x and https://b.example/y");

    let opener = opener.to_str().unwrap();
    rig.open(PANE, &["-n", "-r", r"\w+://[^ ]+", "-e", opener]);
    rig.keys(&["Left", "Enter"]);
    let url = rig.wait_for("program to run", || {
        fs::read_to_string(&opened)
            .ok()
            .filter(|text| text.ends_with('\n'))
    });
    assert_eq!(url, "https://a.example/x\n");
}

#[test]
fn window_mode() {
    let rig = Rig::start(80, 8);
    rig.run(PANE, "echo wind_up wind_down");
    rig.type_text(PANE, "echo wi");

    rig.open(PANE, &["--window"]);
    assert_eq!(
        rig.inner(&["list-windows", "-F", "#{window_index}"])
            .lines()
            .count(),
        2
    );
    rig.keys(&["Enter"]);
    rig.wait_for_line(PANE, "$ echo wind_down");
}

#[test]
fn completes_on_wrapped_line_in_narrow_pane() {
    // The Go version used the window width here and found nothing.
    let rig = Rig::start(100, 10);
    let narrow = rig.split(PANE, false, Some(30));
    rig.run(&narrow, "echo wrap_target");
    rig.type_text(&narrow, "echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa wrap");
    rig.inner(&["select-pane", "-t", &narrow]);

    rig.trigger(&narrow, &[]);
    rig.wait_for_line(&narrow, "$ echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa wrap_target");
}

#[test]
fn regexps_with_shell_characters_survive() {
    let rig = Rig::start(80, 8);
    rig.run(PANE, "echo 'price$5 price$6'");
    rig.type_text(PANE, "echo pr");

    // Candidates must end in a digit: `$` and `#` pass through intact.
    rig.open(PANE, &["-r", r"ice\$[0-9](?:#?)"]);
    rig.keys(&["Enter"]);
    rig.wait_for_line(PANE, "$ echo price$6");
}

#[test]
fn worker_errors_are_reported() {
    let rig = Rig::start(80, 8);
    rig.type_text(PANE, "echo ftp://x.example/f");
    let output = rig.trigger_foreground(PANE, &["-n", "-r", "ftp://[^ ]+", "-e", "/bin/false"]);
    assert!(
        output.contains("/bin/false ftp://x.example/f failed"),
        "output: {output}"
    );
}

#[test]
fn unknown_theme_is_reported() {
    let rig = Rig::start(80, 8);
    let output = rig.trigger_foreground(PANE, &["--theme", "missing"]);
    assert!(
        output.contains(r#"no theme named "missing""#),
        "output: {output}"
    );
}
