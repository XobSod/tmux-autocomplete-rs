//! Thin wrapper around the `tmux` command line client.

use std::ffi::OsStr;
use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Name of the paste buffer used to hand the completion over to tmux, so the
/// user's own buffers are left alone.
const BUFFER_NAME: &str = "tmux-autocomplete";

/// Expands a tmux format string, optionally in the context of `target`.
pub fn display(target: Option<&str>, format: &str) -> Result<String> {
    let mut args = vec!["display-message", "-p"];
    if let Some(target) = target {
        args.extend(["-t", target]);
    }
    args.push(format);

    let output = run(&args, None)?;
    Ok(output.trim_end_matches('\n').to_owned())
}

/// Returns the visible contents of `target`, including SGR escape sequences,
/// with wrapped lines joined.
pub fn capture_pane(target: &str) -> Result<String> {
    run(&["capture-pane", "-p", "-e", "-J", "-t", target], None)
}

/// Opens a new window running `program` with `args`. tmux executes the
/// command directly, without a shell, because it is given as several
/// arguments.
pub fn new_window<I, S>(program: &OsStr, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut argv: Vec<&OsStr> = vec!["new-window".as_ref(), program];
    let args: Vec<S> = args.into_iter().collect();
    argv.extend(args.iter().map(AsRef::as_ref));
    run(&argv, None).map(drop)
}

/// Pastes `text` into `target` as if it had been typed.
pub fn paste(target: &str, text: &str) -> Result<()> {
    run(
        &["load-buffer", "-b", BUFFER_NAME, "-"],
        Some(text.as_bytes()),
    )?;
    run(
        &["paste-buffer", "-d", "-b", BUFFER_NAME, "-t", target],
        None,
    )
    .map(drop)
}

fn run<S: AsRef<OsStr>>(args: &[S], stdin: Option<&[u8]>) -> Result<String> {
    let describe = || {
        let args: Vec<_> = args.iter().map(|a| a.as_ref().to_string_lossy()).collect();
        format!("tmux {}", args.join(" "))
    };

    let mut child = Command::new("tmux")
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("unable to run {}", describe()))?;

    if let Some(input) = stdin {
        // Dropping the handle closes the pipe so tmux sees end of input.
        child
            .stdin
            .take()
            .expect("stdin is piped")
            .write_all(input)
            .with_context(|| format!("unable to write to {}", describe()))?;
    }

    let output = child
        .wait_with_output()
        .with_context(|| format!("unable to run {}", describe()))?;

    if !output.status.success() {
        bail!(
            "{} failed ({}): {}",
            describe(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    // Pane contents can hold arbitrary bytes; never fail on them.
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
