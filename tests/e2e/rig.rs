//! A nested tmux test rig: an outer server shows a terminal attached to an
//! inner server, so what the inner client displays (popups included) can be
//! captured, and keys can be typed into it.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use tempfile::TempDir;

const TIMEOUT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(20);

/// Colours of the test theme, chosen to be unique on screen.
pub const IDENTIFIER: u8 = 46;
pub const NORMAL: u8 = 21;
pub const SELECTED: u8 = 201;
pub const FOG: u8 = 244;
pub const PROMPT: u8 = 226;

const THEME: &str = "\
identifier: 46
candidate:
    normal: 21
    selected: 201
fog:
    text: 244
    background: 245:236
prompt: 226
";

pub const BIN: &str = env!("CARGO_BIN_EXE_tmux-autocomplete");

pub struct Rig {
    /// Holds the sockets, configuration and test theme; removed on drop.
    dir: TempDir,
}

impl Rig {
    /// Starts a client of `width` × `height` cells (the status line takes
    /// the top row) showing a bash prompt `$ ` in pane `%0`.
    pub fn start(width: usize, height: usize) -> Self {
        Self::start_with(width, height, "")
    }

    /// Like `start`, with extra configuration for the inner server.
    pub fn start_with(width: usize, height: usize, config: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("test.theme"), THEME).unwrap();
        fs::write(
            dir.path().join("inner.conf"),
            format!(
                "set -g default-terminal tmux-256color\nset -g status-position top\n\
                 set -g history-limit 5000\nset -g escape-time 0\n{config}\n"
            ),
        )
        .unwrap();
        fs::write(
            dir.path().join("outer.conf"),
            "set -g default-terminal tmux-256color\nset -g status off\nset -g escape-time 0\n",
        )
        .unwrap();

        let rig = Self { dir };
        rig.inner(&[
            "new-session",
            "-d",
            "-s",
            "t",
            "-x",
            &width.to_string(),
            "-y",
            &(height - 1).to_string(),
            &shell_command(),
        ]);
        let attach = format!(
            "env -u TMUX TERM=tmux-256color tmux -S {} -f {} attach -t t",
            rig.path("inner.sock").display(),
            rig.path("inner.conf").display()
        );
        rig.outer(&[
            "new-session",
            "-d",
            "-s",
            "o",
            "-x",
            &width.to_string(),
            "-y",
            &height.to_string(),
            &attach,
        ]);
        rig.wait_for("client to attach", || {
            (!rig
                .inner(&["list-clients", "-F", "#{client_name}"])
                .trim()
                .is_empty())
            .then_some(())
        });
        rig.wait_for_prompt("%0");
        rig
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// Runs a command on the inner server and returns its output.
    pub fn inner(&self, args: &[&str]) -> String {
        self.tmux("inner", args)
    }

    fn outer(&self, args: &[&str]) -> String {
        self.tmux("outer", args)
    }

    /// Runs tmux on the `server` ("inner" or "outer") socket.
    fn tmux(&self, server: &str, args: &[&str]) -> String {
        let output: Output = Command::new("tmux")
            .arg("-S")
            .arg(self.path(&format!("{server}.sock")))
            .arg("-f")
            .arg(self.path(&format!("{server}.conf")))
            .args(args)
            .env_remove("TMUX")
            .output()
            .expect("tmux must be installed to run the end-to-end tests");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Opens a pane next to `pane` (to its right, or below if `vertical`)
    /// with a bash prompt and returns its id.
    pub fn split(&self, pane: &str, vertical: bool, size: Option<usize>) -> String {
        let size = size.map(|s| s.to_string());
        let mut args = vec![
            "split-window",
            if vertical { "-v" } else { "-h" },
            "-d",
            "-P",
            "-F",
            "#{pane_id}",
            "-t",
            pane,
        ];
        if let Some(size) = &size {
            args.extend(["-l", size]);
        }
        let command = shell_command();
        args.push(&command);
        let id = self.inner(&args).trim().to_owned();
        self.wait_for_prompt(&id);
        id
    }

    /// Runs `command` in the shell of `pane` and waits for the next prompt.
    pub fn run(&self, pane: &str, command: &str) {
        let before = self.prompt_count(pane);
        self.inner(&["send-keys", "-t", pane, "-l", command]);
        self.inner(&["send-keys", "-t", pane, "Enter"]);
        self.wait_for(&format!("{command:?} to finish in {pane}"), || {
            (self.prompt_count(pane) > before && self.last_line(pane) == "$").then_some(())
        });
    }

    /// Types `text` at the prompt of `pane` without running it.
    pub fn type_text(&self, pane: &str, text: &str) {
        self.inner(&["send-keys", "-t", pane, "-l", text]);
        let expected = format!("$ {text}");
        self.wait_for(&format!("{text:?} to appear in {pane}"), || {
            (self.last_line(pane) == expected).then_some(())
        });
    }

    /// Starts a completion in `pane`, which must be the active pane (as it
    /// is for a key binding), with the test theme and `args`.
    pub fn trigger(&self, pane: &str, args: &[&str]) {
        self.inner(&["run-shell", "-b", "-t", pane, &self.command_line(args)]);
    }

    /// Like `trigger`, and waits for the picker to be shown.
    pub fn open(&self, pane: &str, args: &[&str]) -> View {
        self.trigger(pane, args);
        self.wait_view("picker to open", View::is_picker)
    }

    /// Runs a completion in the foreground, as tmux does for `run-shell`
    /// without `-b`, and returns what it printed.
    pub fn trigger_foreground(&self, pane: &str, args: &[&str]) -> String {
        let line = self.command_line(args);
        let script = format!("{line} > {} 2>&1", self.path("output").display());
        self.inner(&["run-shell", "-t", pane, &script]);
        fs::read_to_string(self.path("output")).unwrap_or_default()
    }

    fn command_line(&self, args: &[&str]) -> String {
        let theme_dir = self.dir.path().to_str().unwrap();
        let mut words = vec![BIN, "--theme-path", theme_dir];
        if !args.contains(&"--theme") {
            words.extend(["--theme", "test"]);
        }
        words.extend(args);
        words.iter().map(|w| quote(w)).collect::<Vec<_>>().join(" ")
    }

    /// Types tmux key names (`Enter`, `Left`, `j`, ...) into the client.
    pub fn keys(&self, keys: &[&str]) {
        let mut args = vec!["send-keys", "-t", "o:0"];
        args.extend(keys);
        self.outer(&args);
    }

    /// What the client shows now.
    pub fn view(&self) -> View {
        View::parse(&self.outer(&["capture-pane", "-p", "-e", "-t", "o:0"]))
    }

    /// Waits until `probe` accepts the view and returns it.
    pub fn wait_view(&self, what: &str, probe: impl Fn(&View) -> bool) -> View {
        self.wait_for(what, || {
            let view = self.view();
            probe(&view).then_some(view)
        })
    }

    /// Waits until the picker is gone and `pane` shows the prompt `line`.
    pub fn wait_for_line(&self, pane: &str, line: &str) {
        self.wait_for(&format!("prompt {line:?} in {pane}"), || {
            (!self.view().is_picker() && self.last_line(pane) == line).then_some(())
        });
    }

    /// Last non-empty line of `pane`.
    pub fn last_line(&self, pane: &str) -> String {
        let text = self.inner(&["capture-pane", "-p", "-J", "-t", pane]);
        text.lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim_end()
            .to_owned()
    }

    fn prompt_count(&self, pane: &str) -> usize {
        let text = self.inner(&["capture-pane", "-p", "-J", "-S", "-", "-t", pane]);
        text.lines().filter(|l| l.starts_with('$')).count()
    }

    fn wait_for_prompt(&self, pane: &str) {
        self.wait_for(&format!("prompt in {pane}"), || {
            (self.last_line(pane) == "$").then_some(())
        });
    }

    pub fn wait_for<T>(&self, what: &str, mut probe: impl FnMut() -> Option<T>) -> T {
        let start = Instant::now();
        loop {
            if let Some(value) = probe() {
                return value;
            }
            if start.elapsed() > TIMEOUT {
                panic!(
                    "timed out waiting for {what}; screen:\n{}",
                    self.view().text()
                );
            }
            std::thread::sleep(POLL);
        }
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.outer(&["kill-server"]);
        let _ = self.inner(&["kill-server"]);
    }
}

fn shell_command() -> String {
    format!(
        "env -i HOME={} PATH=/usr/local/bin:/usr/bin:/bin TERM=tmux-256color PS1='$ ' bash --norc --noprofile",
        quote(&std::env::var("HOME").unwrap_or_default())
    )
}

/// Quotes a word for a POSIX shell.
pub fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// A character on screen with the parts of its style the tests look at.
#[derive(Debug, Clone, Copy)]
struct Cell {
    ch: char,
    fg: Option<u8>,
    underline: bool,
}

/// The client screen, decoded from `capture-pane -e`.
pub struct View {
    rows: Vec<Vec<Cell>>,
}

impl View {
    fn parse(captured: &str) -> Self {
        let mut fg = None;
        let mut underline = false;
        let rows = captured
            .lines()
            .map(|line| {
                let mut row = Vec::new();
                let mut chars = line.chars().peekable();
                while let Some(ch) = chars.next() {
                    if ch != '\x1b' {
                        row.push(Cell { ch, fg, underline });
                        continue;
                    }
                    if chars.next() != Some('[') {
                        continue;
                    }
                    let params: String = chars.by_ref().take_while(|&c| c != 'm').collect();
                    let codes: Vec<u16> = params
                        .split([';', ':'])
                        .map(|p| p.parse().unwrap_or(0))
                        .collect();
                    let mut codes = codes.into_iter();
                    while let Some(code) = codes.next() {
                        match code {
                            0 => (fg, underline) = (None, false),
                            4 => underline = true,
                            24 => underline = false,
                            39 => fg = None,
                            38 => {
                                if codes.next() == Some(5) {
                                    fg = codes.next().and_then(|c| u8::try_from(c).ok());
                                }
                            }
                            48 => {
                                if codes.next() == Some(5) {
                                    codes.next();
                                }
                            }
                            30..=37 | 90..=97 => fg = u8::try_from(code).ok(),
                            _ => {}
                        }
                    }
                }
                row
            })
            .collect();
        Self { rows }
    }

    pub fn text(&self) -> String {
        self.rows
            .iter()
            .map(|row| row.iter().map(|c| c.ch).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Whether the picker is on screen (the dimmed pane text is there).
    pub fn is_picker(&self) -> bool {
        self.rows
            .iter()
            .flatten()
            .any(|c| c.fg == Some(FOG) && !c.ch.is_whitespace())
    }

    /// Runs of text drawn in colour `fg`, in reading order.
    pub fn runs(&self, fg: u8) -> Vec<String> {
        let mut runs = Vec::new();
        for row in &self.rows {
            let mut run = String::new();
            for cell in row {
                if cell.fg == Some(fg) {
                    run.push(cell.ch);
                } else if !run.is_empty() {
                    runs.push(std::mem::take(&mut run));
                }
            }
            if !run.is_empty() {
                runs.push(run);
            }
        }
        runs
    }

    pub fn selected(&self) -> Option<String> {
        self.runs(SELECTED).into_iter().next()
    }

    /// All highlighted candidates, selected one included, in reading order.
    pub fn candidates(&self) -> Vec<String> {
        let mut runs: Vec<(usize, usize, String)> = Vec::new();
        for (y, row) in self.rows.iter().enumerate() {
            let mut start = None;
            for (x, cell) in row
                .iter()
                .chain(std::iter::once(&Cell {
                    ch: ' ',
                    fg: None,
                    underline: false,
                }))
                .enumerate()
            {
                let highlighted = matches!(cell.fg, Some(NORMAL | SELECTED));
                match (highlighted, start) {
                    (true, None) => start = Some(x),
                    (false, Some(s)) => {
                        runs.push((y, s, row[s..x].iter().map(|c| c.ch).collect()));
                        start = None;
                    }
                    _ => {}
                }
            }
        }
        runs.into_iter().map(|(_, _, text)| text).collect()
    }

    /// Characters of the selected candidate that are underlined.
    pub fn selected_underlined(&self) -> String {
        self.rows
            .iter()
            .flatten()
            .filter(|c| c.fg == Some(SELECTED) && c.underline)
            .map(|c| c.ch)
            .collect()
    }

    /// The filter prompt on the last row, if shown.
    pub fn prompt(&self) -> Option<String> {
        let last = self
            .rows
            .iter()
            .rev()
            .find(|row| row.iter().any(|c| c.fg == Some(PROMPT)))?;
        let text: String = last
            .iter()
            .filter(|c| c.fg == Some(PROMPT))
            .map(|c| c.ch)
            .collect();
        text.starts_with('/').then(|| text.trim_end().to_owned())
    }

    /// Scroll indicators such as `[3/40]`.
    pub fn indicators(&self) -> Vec<String> {
        self.runs(PROMPT)
            .into_iter()
            .filter(|r| r.starts_with('['))
            .collect()
    }
}
