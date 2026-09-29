//! The two halves of a completion: the launcher, started by the tmux key
//! binding, and the worker, which the launcher runs in a popup (or a new
//! window) to show the picker.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use anyhow::{Context, Result, bail};
use log::debug;
use nix::sys::stat::Mode;
use nix::unistd::mkfifo;

use crate::cli::Cli;
use crate::completion::{self, Candidate, Word};
use crate::layout::Cell;
use crate::picker::{Outcome, Picker};
use crate::theme::{self, Theme};
use crate::tmux::{self, PopupArea};
use crate::ui;
use crate::window::{self, History};

/// Loads the theme selected on the command line.
pub fn load_theme(cli: &Cli) -> Result<Theme> {
    let dirs = theme::theme_dirs(cli.theme_path.as_deref())?;
    Theme::load(&dirs, &cli.theme)
}

/// What tmux reports about the place the completion was started from.
struct Origin {
    pane: String,
    cursor_x: String,
    cursor_y: String,
    client: String,
    /// Area of the window on the client.
    area: PopupArea,
}

impl Origin {
    fn query() -> Result<Self> {
        let reply = tmux::display(
            None,
            "#{pane_id}\t#{cursor_x}\t#{cursor_y}\t#{client_name}\t#{client_width}\t\
             #{client_height}\t#{window_width}\t#{window_height}\t#{status-position}",
        )
        .context("unable to get current pane and cursor")?;
        let fields: Vec<&str> = reply.split('\t').collect();
        let [
            pane,
            cursor_x,
            cursor_y,
            client,
            client_width,
            client_height,
            width,
            height,
            status,
        ] = fields[..]
        else {
            bail!("unexpected reply from tmux: {reply:?}");
        };
        let number = |value: &str| {
            value
                .parse::<usize>()
                .with_context(|| format!("unexpected reply from tmux: {reply:?}"))
        };

        let client_height = number(client_height)?;
        let height = number(height)?.min(client_height);
        // The status line takes the rows the window doesn't.
        let top = if status == "top" {
            client_height - height
        } else {
            0
        };
        Ok(Self {
            pane: pane.to_owned(),
            cursor_x: cursor_x.to_owned(),
            cursor_y: cursor_y.to_owned(),
            client: client.to_owned(),
            area: PopupArea {
                x: 0,
                y: top,
                width: number(width)?.min(number(client_width)?),
                height,
            },
        })
    }
}

/// Starts the worker for the active pane and waits for it to finish.
/// Returns an error with whatever the worker reported.
pub fn launch(cli: &Cli) -> Result<()> {
    let origin = Origin::query()?;
    debug!(
        "origin: pane {} client {} area {:?}",
        origin.pane, origin.client, origin.area
    );

    let dir = tempfile::Builder::new()
        .prefix("tmux-autocomplete-")
        .tempdir()
        .context("unable to create temporary directory")?;
    let report_path = dir.path().join("report");

    let mut args = cli.forwarded_args();
    args.extend(
        [
            "--report-to".as_ref(),
            report_path.as_os_str(),
            "--client".as_ref(),
            origin.client.as_ref(),
            "-W".as_ref(),
            origin.pane.as_ref(),
            origin.cursor_x.as_ref(),
            origin.cursor_y.as_ref(),
        ]
        .map(OsString::from),
    );
    let program = env::current_exe().context("unable to find own executable")?;
    debug!("starting worker: {} {args:?}", program.display());

    if cli.window {
        // The worker's window closes when it exits and new-window doesn't
        // wait, so the report is a FIFO: reading it waits for the worker.
        mkfifo(&report_path, Mode::S_IRUSR | Mode::S_IWUSR).context("unable to create FIFO")?;
        tmux::new_window(program.as_os_str(), &args).context("unable to create tmux window")?;
    } else {
        File::create(&report_path).context("unable to create report file")?;
        let command = shell_command(&program, &args);
        let popup = tmux::display_popup(&origin.client, &origin.pane, origin.area, &command);
        // A failing worker also fails the popup; its own report says more.
        read_report(&report_path)?;
        popup.context("unable to open popup (tmux 3.3 or later is needed, or use --window)")?;
        return Ok(());
    }

    read_report(&report_path)
}

fn read_report(path: &Path) -> Result<()> {
    let report = fs::read_to_string(path).context("unable to read worker report")?;
    let report = report.trim();
    if !report.is_empty() {
        bail!("{report}");
    }
    Ok(())
}

/// A POSIX shell command line running `program` with `args`.
fn shell_command(program: &Path, args: &[OsString]) -> OsString {
    let mut command = b"exec".to_vec();
    for word in std::iter::once(program.as_os_str()).chain(args.iter().map(OsString::as_os_str)) {
        command.push(b' ');
        command.extend(shell_quote(word));
    }
    OsString::from_vec(command)
}

fn shell_quote(word: &OsStr) -> Vec<u8> {
    let mut quoted = vec![b'\''];
    for &byte in word.as_bytes() {
        if byte == b'\'' {
            quoted.extend(b"'\\''");
        } else {
            quoted.push(byte);
        }
    }
    quoted.push(b'\'');
    quoted
}

/// Where the worker should complete.
#[derive(Debug)]
pub struct Target {
    pub pane: String,
    pub cursor: Cell,
}

impl Target {
    pub fn from_args(args: &[String]) -> Result<Self> {
        let [pane, x, y] = args else {
            bail!("expected PANE CURSOR_X CURSOR_Y");
        };
        let parse = |value: &str| {
            value
                .parse::<usize>()
                .with_context(|| format!("invalid cursor position {value:?}"))
        };
        Ok(Self {
            pane: pane.clone(),
            cursor: Cell {
                row: parse(y)?,
                col: parse(x)?,
            },
        })
    }
}

/// Runs the completion for `target`: finds candidates in the panes of its
/// window, lets the user pick one, then inserts or copies it.
pub fn run_worker(cli: &Cli, target: &Target) -> Result<()> {
    let theme = load_theme(cli)?;
    let history = cli.history.map_or(History::All, History::Lines);
    let (panes, active) =
        window::capture(&target.pane, history).context("unable to capture panes")?;

    let Some((line, column)) = panes[active].locate_on_screen(target.cursor) else {
        debug!("cursor {:?} is below the pane contents", target.cursor);
        return Ok(());
    };

    let identifier = if cli.no_prefix {
        None
    } else {
        let text = &panes[active].lines[line].text;
        match completion::find_identifier(&cli.regexp_cursor, text, active, line, column)? {
            Some(identifier) => Some(identifier),
            None => {
                debug!("nothing to complete at line {line}, column {column}");
                return Ok(());
            }
        }
    };
    debug!("identifier: {identifier:?}");

    let matcher = completion::candidate_matcher(&cli.regexp_candidate, identifier.as_ref())?;
    let mut candidates = Vec::new();
    for (index, pane) in panes.iter().enumerate() {
        completion::find_candidates(
            &matcher,
            index,
            &pane.lines,
            identifier.as_ref(),
            &mut candidates,
        );
    }
    // Of duplicates, keep the one on screen, preferably in the active pane.
    let candidates = completion::dedup(candidates, |c| {
        let pane = &panes[c.word.pane];
        let on_screen = pane.cell(c.word.line, c.word.column).row >= pane.screen_top;
        (on_screen, c.word.pane == active)
    });
    debug!("{} candidates", candidates.len());
    if candidates.is_empty() {
        return Ok(());
    }

    let mut picker = Picker::new(panes, active, identifier, candidates, (line, column));
    let outcome = match (picker.candidates.len(), picker.selected()) {
        (1, Some(only)) => Outcome::Insert(only),
        _ => ui::run(&mut picker, &theme)?,
    };
    debug!("outcome: {outcome:?}");

    match outcome {
        Outcome::Insert(index) => insert(
            cli,
            target,
            picker.identifier.as_ref(),
            &picker.candidates[index],
        ),
        Outcome::Copy(index) => copy(cli, &picker.candidates[index].word.value),
        Outcome::Continue | Outcome::Cancel => Ok(()),
    }
}

fn insert(
    cli: &Cli,
    target: &Target,
    identifier: Option<&Word>,
    candidate: &Candidate,
) -> Result<()> {
    let value = &candidate.word.value;

    if let Some(program) = &cli.exec {
        let status = Command::new(program)
            .arg(value)
            .status()
            .with_context(|| format!("unable to run {}", PathBuf::from(program).display()))?;
        if !status.success() {
            bail!(
                "{} {value} failed: {status}",
                PathBuf::from(program).display()
            );
        }
        return Ok(());
    }

    // Only the part after the identifier is typed.
    let suffix: String = value
        .chars()
        .skip(identifier.map_or(0, Word::len))
        .collect();
    tmux::paste(&target.pane, &suffix)
}

fn copy(cli: &Cli, value: &str) -> Result<()> {
    let client = cli.client.as_deref().context("no client to copy for")?;
    tmux::copy(client, value)?;
    tmux::message(client, &format!("Copied: {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_for_the_shell() {
        let command = shell_command(
            Path::new("/opt/my app/tac"),
            &["it's".into(), "#{x} $y".into()],
        );
        assert_eq!(
            command,
            OsString::from(r#"exec '/opt/my app/tac' 'it'\''s' '#{x} $y'"#)
        );
    }
}
