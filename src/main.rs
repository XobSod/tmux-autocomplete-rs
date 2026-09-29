use std::fs::OpenOptions;
use std::io::{self, IsTerminal};
use std::process::ExitCode;

use clap::Parser;

use tmux_autocomplete::app::{self, Target};
use tmux_autocomplete::cli::Cli;
use tmux_autocomplete::{crash, logging};

const INTRO: &str = "\
tmux-autocomplete is meant to be started from a tmux key binding. Add the
following line to ~/.tmux.conf:

  bind-key C-Space run -b 'tmux-autocomplete'

then reload the configuration:

  tmux source ~/.tmux.conf

Completion is now triggered with Control+b Control+Space.";

fn main() -> ExitCode {
    let cli = Cli::parse();

    if let Some(path) = &cli.debug
        && let Err(err) = logging::init(path)
    {
        eprintln!("{err:#}");
        return ExitCode::from(1);
    }

    match &cli.worker {
        Some(args) => worker(&cli, args),
        None => launcher(&cli),
    }
}

fn launcher(cli: &Cli) -> ExitCode {
    if io::stdin().is_terminal() {
        println!("{INTRO}");
        return ExitCode::from(1);
    }

    // tmux shows the stdout of `run-shell` commands, so errors go there.
    if let Err(err) = app::load_theme(cli) {
        println!("unable to load theme: {err:#}");
        return ExitCode::from(2);
    }
    if let Err(err) = app::launch(cli) {
        log::error!("{err:#}");
        println!("tmux-autocomplete: {err:#}");
        return ExitCode::from(3);
    }
    ExitCode::SUCCESS
}

fn worker(cli: &Cli, args: &[String]) -> ExitCode {
    // Open the report first: the launcher waits for it.
    if let Some(path) = &cli.report_to {
        match OpenOptions::new().write(true).open(path) {
            Ok(report) => crash::report_to(report),
            Err(err) => eprintln!("unable to open {}: {err}", path.display()),
        }
    }

    let result = Target::from_args(args).and_then(|target| app::run_worker(cli, &target));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            log::error!("{err:#}");
            crash::report_error(&format!("{err:#}"));
            ExitCode::from(1)
        }
    }
}
