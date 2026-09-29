//! The two halves of a completion: the launcher, started by the tmux key
//! binding, and the worker, which the launcher runs in a new tmux window to
//! show the picker.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

use anyhow::{Context, Result, bail};
use log::debug;
use nix::sys::stat::Mode;
use nix::unistd::mkfifo;

use crate::cli::Cli;
use crate::completion::{self, Candidate, Word};
use crate::layout::{Cell, Layout};
use crate::pane;
use crate::theme::{self, Theme};
use crate::tmux;
use crate::ui::Picker;

/// Loads the theme selected on the command line.
pub fn load_theme(cli: &Cli) -> Result<Theme> {
    let dirs = theme::theme_dirs(cli.theme_path.as_deref())?;
    Theme::load(&dirs, &cli.theme)
}

/// Starts the worker for the active pane and waits for it to finish.
/// Returns an error with whatever the worker reported.
pub fn launch(cli: &Cli) -> Result<()> {
    let active = tmux::display(None, "#{pane_id}\t#{cursor_x}\t#{cursor_y}")
        .context("unable to get current pane and cursor")?;
    let [pane, cursor_x, cursor_y] = active.split('\t').collect::<Vec<_>>()[..] else {
        bail!("unexpected reply from tmux: {active:?}");
    };

    // The worker's window closes when it exits, so it reports errors back
    // through a FIFO. Reading it also waits for the worker to finish.
    let dir = tempfile::Builder::new()
        .prefix("tmux-autocomplete-")
        .tempdir()
        .context("unable to create temporary directory")?;
    let report_path = dir.path().join("report");
    mkfifo(&report_path, Mode::S_IRUSR | Mode::S_IWUSR).context("unable to create FIFO")?;

    let mut args = cli.forwarded_args();
    args.extend(
        [
            "--report-to".as_ref(),
            report_path.as_os_str(),
            "-W".as_ref(),
            pane.as_ref(),
            cursor_x.as_ref(),
            cursor_y.as_ref(),
        ]
        .map(OsString::from),
    );

    let program = env::current_exe().context("unable to find own executable")?;
    debug!("starting worker: {program:?} {args:?}");
    tmux::new_window(program.as_os_str(), &args).context("unable to create tmux window")?;

    let report = fs::read_to_string(&report_path).context("unable to read worker report")?;
    let report = report.trim();
    if !report.is_empty() {
        bail!("{report}");
    }
    Ok(())
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

/// Runs the completion for `target`: finds candidates, lets the user pick
/// one, then pastes it into the pane or passes it to `--exec`.
pub fn run_worker(cli: &Cli, target: &Target) -> Result<()> {
    let theme = load_theme(cli)?;

    let captured = tmux::capture_pane(&target.pane).context("unable to capture pane")?;
    let lines = pane::parse(&captured);
    let pane_width: usize = tmux::display(Some(&target.pane), "#{pane_width}")?
        .parse()
        .context("invalid pane width")?;

    // The cursor position is in cells of the original pane, which can be
    // narrower than this window.
    let Some((line, column)) = Layout::new(&lines, pane_width).locate(target.cursor) else {
        debug!("cursor {:?} is below the pane contents", target.cursor);
        return Ok(());
    };

    let identifier = if cli.no_prefix {
        None
    } else {
        match completion::find_identifier(&cli.regexp_cursor, &lines[line].text, line, column)? {
            Some(identifier) => Some(identifier),
            None => {
                debug!("nothing to complete at line {line}, column {column}");
                return Ok(());
            }
        }
    };
    debug!("identifier: {identifier:?}");

    let candidates = completion::dedup(completion::find_candidates(
        &cli.regexp_candidate,
        &lines,
        identifier.as_ref(),
    )?);
    for candidate in &candidates {
        debug!("candidate: {candidate:?}");
    }
    let Some(last) = candidates.last() else {
        return Ok(());
    };

    // Without a prefix the last candidate stands in for the identifier.
    let anchor = identifier.clone().unwrap_or_else(|| last.word.clone());
    let selected = completion::default_selection(&candidates, &anchor);

    let chosen = if candidates.len() == 1 {
        Some(selected)
    } else {
        Picker {
            lines: &lines,
            theme: &theme,
            identifier: &anchor,
            candidates: &candidates,
        }
        .run(selected)?
    };

    match chosen {
        Some(index) => accept(cli, target, identifier.as_ref(), &candidates[index]),
        None => Ok(()),
    }
}

fn accept(
    cli: &Cli,
    target: &Target,
    identifier: Option<&Word>,
    candidate: &Candidate,
) -> Result<()> {
    let value = &candidate.word.value;
    debug!("accepted: {value:?}");

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
