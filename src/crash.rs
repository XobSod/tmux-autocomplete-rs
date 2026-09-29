//! Crash reports for panics in the worker, whose window disappears together
//! with anything printed to it.

use std::backtrace::Backtrace;
use std::env;
use std::fs::File;
use std::io::Write;
use std::panic::{self, PanicHookInfo};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Where the worker reports errors to the launcher.
static REPORT: OnceLock<File> = OnceLock::new();

/// Sends errors to `report` from now on, including panics.
pub fn report_to(report: File) {
    let _ = REPORT.set(report);
    panic::set_hook(Box::new(on_panic));
}

/// Reports an error to the launcher, or to stderr if there is none.
pub fn report_error(message: &str) {
    match REPORT.get() {
        Some(mut report) => {
            let _ = writeln!(report, "{message}");
        }
        None => eprintln!("{message}"),
    }
}

fn on_panic(info: &PanicHookInfo) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = env::temp_dir().join(format!("tmux-autocomplete-panic-{timestamp}.log"));
    let contents = format!(
        "tmux-autocomplete {} crashed\n\n{info}\n\n{}\n",
        env!("CARGO_PKG_VERSION"),
        Backtrace::force_capture()
    );

    let saved = File::create(&path).and_then(|mut file| file.write_all(contents.as_bytes()));
    log::error!("{info}");
    report_error(&match saved {
        Ok(()) => format!(
            "tmux-autocomplete crashed: {info}\nReport saved to {}",
            path.display()
        ),
        Err(_) => contents,
    });
}
