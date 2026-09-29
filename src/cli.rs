use std::ffi::OsString;
use std::path::PathBuf;

use clap::{ArgAction, Parser};
use regex::Regex;

pub const DEFAULT_REGEXP_CURSOR: &str = "[!-~]+";
pub const DEFAULT_REGEXP_CANDIDATE: &str = "[^<> ]+";

/// Provides an autocomplete interface for pane contents.
///
/// Meant to be started from a tmux key binding, for example:
///
///   bind-key C-Space run -b 'tmux-autocomplete'
#[derive(Debug, Parser)]
#[command(name = "tmux-autocomplete", version, disable_version_flag = true)]
pub struct Cli {
    /// Regexp matching the identifier before the cursor.
    #[arg(
        short = 'c',
        long,
        value_name = "REGEXP",
        default_value = DEFAULT_REGEXP_CURSOR,
        value_parser = parse_regex,
    )]
    pub regexp_cursor: Regex,

    /// Regexp matching candidates (appended to the identifier).
    #[arg(
        short = 'r',
        long,
        value_name = "REGEXP",
        default_value = DEFAULT_REGEXP_CANDIDATE,
        value_parser = parse_regex,
    )]
    pub regexp_candidate: Regex,

    /// Don't use the identifier under the cursor as a prefix.
    #[arg(short = 'n', long)]
    pub no_prefix: bool,

    /// Run PROGRAM with the selected candidate as its argument instead of
    /// pasting it.
    #[arg(short = 'e', long, value_name = "PROGRAM")]
    pub exec: Option<OsString>,

    /// Name of the theme to use.
    #[arg(long, value_name = "NAME", default_value = "light")]
    pub theme: String,

    /// Directories to search for themes, separated by ':'.
    ///
    /// Defaults to ~/.config/tmux-autocomplete/themes/ followed by
    /// /usr/share/tmux-autocomplete/themes/. The built-in light and dark
    /// themes are used when no file is found.
    #[arg(long, value_name = "DIRS")]
    pub theme_path: Option<String>,

    /// Show the picker in a new window instead of a popup (for tmux older
    /// than 3.3).
    #[arg(long)]
    pub window: bool,

    /// Lines of scrollback to search in each pane. Defaults to all of it.
    #[arg(long, value_name = "LINES")]
    pub history: Option<usize>,

    /// Append debug messages to FILE.
    #[arg(long, value_name = "FILE")]
    pub debug: Option<PathBuf>,

    /// Print version.
    #[arg(short = 'v', long, action = ArgAction::Version)]
    version: Option<bool>,

    /// Internal: run the interactive picker for PANE with the cursor at
    /// CURSOR_X, CURSOR_Y.
    #[arg(
        short = 'W',
        long = "worker",
        num_args = 3,
        value_names = ["PANE", "CURSOR_X", "CURSOR_Y"],
        hide = true,
    )]
    pub worker: Option<Vec<String>>,

    /// Internal: file or FIFO the worker reports errors to.
    #[arg(long, value_name = "PATH", hide = true, requires = "worker")]
    pub report_to: Option<PathBuf>,

    /// Internal: the tmux client the completion was started from.
    #[arg(long, value_name = "NAME", hide = true, requires = "worker")]
    pub client: Option<String>,
}

impl Cli {
    /// Arguments that reproduce the user-facing options of this invocation,
    /// used to start the worker with the same configuration.
    pub fn forwarded_args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![
            "--regexp-cursor".into(),
            self.regexp_cursor.as_str().into(),
            "--regexp-candidate".into(),
            self.regexp_candidate.as_str().into(),
            "--theme".into(),
            self.theme.clone().into(),
        ];
        if self.no_prefix {
            args.push("--no-prefix".into());
        }
        if let Some(program) = &self.exec {
            args.extend(["--exec".into(), program.clone()]);
        }
        if let Some(dirs) = &self.theme_path {
            args.extend(["--theme-path".into(), dirs.into()]);
        }
        if let Some(lines) = self.history {
            args.extend(["--history".into(), lines.to_string().into()]);
        }
        if let Some(file) = &self.debug {
            args.extend(["--debug".into(), file.into()]);
        }
        args
    }
}

fn parse_regex(pattern: &str) -> Result<Regex, regex::Error> {
    Regex::new(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_args_round_trip() {
        let cli = Cli::parse_from([
            "tmux-autocomplete",
            "-n",
            "-r",
            r"\w+://[^ ]+$",
            "-e",
            "xdg-open",
            "--theme",
            "dark",
        ]);
        let mut argv: Vec<OsString> = vec!["tmux-autocomplete".into()];
        argv.extend(cli.forwarded_args());
        let again = Cli::parse_from(argv);

        assert!(again.no_prefix);
        assert_eq!(again.regexp_candidate.as_str(), r"\w+://[^ ]+$");
        assert_eq!(again.regexp_cursor.as_str(), DEFAULT_REGEXP_CURSOR);
        assert_eq!(again.exec.as_deref(), Some("xdg-open".as_ref()));
        assert_eq!(again.theme, "dark");
    }

    #[test]
    fn cli_definition_is_valid() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
