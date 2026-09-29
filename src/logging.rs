//! Debug logging to a file, enabled with `--debug`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use log::{LevelFilter, Log, Metadata, Record};

struct FileLogger {
    file: Mutex<File>,
}

impl Log for FileLogger {
    fn enabled(&self, _: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(
            file,
            "{timestamp:.3} [{}] {}:{} {}",
            std::process::id(),
            record.file().unwrap_or("?"),
            record.line().unwrap_or(0),
            record.args()
        );
    }

    fn flush(&self) {
        let _ = self.file.lock().map(|mut file| file.flush());
    }
}

/// Appends log messages of all levels to `path`.
pub fn init(path: &Path) -> Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("unable to open debug log {}", path.display()))?;
    log::set_boxed_logger(Box::new(FileLogger {
        file: Mutex::new(file),
    }))?;
    log::set_max_level(LevelFilter::Debug);
    Ok(())
}
