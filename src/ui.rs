//! The interactive candidate picker drawn over a dimmed copy of the pane.

use std::io::{self, Stdout, Write};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{ContentStyle, PrintStyledContent, StyledContent};
use crossterm::{cursor, queue, terminal};

use crate::completion::{self, Candidate, Direction, Word};
use crate::layout::Layout;
use crate::pane::Line;
use crate::theme::Theme;

pub struct Picker<'a> {
    pub lines: &'a [Line],
    pub theme: &'a Theme,
    pub identifier: &'a Word,
    pub candidates: &'a [Candidate],
}

impl Picker<'_> {
    /// Lets the user choose a candidate, starting at `selected`. Returns
    /// `None` when the user cancels.
    pub fn run(&self, mut selected: usize) -> Result<Option<usize>> {
        let mut screen = Screen::enter()?;

        loop {
            let (width, height) = terminal::size()?;
            let layout = Layout::new(self.lines, width.into());
            self.draw(&mut screen, &layout, height.into(), selected)?;

            let Event::Key(key) = event::read()? else {
                // Redraw, e.g. after a resize.
                continue;
            };
            match action(key) {
                Some(Action::Move(direction)) => {
                    if let Some(next) = completion::navigate(self.candidates, selected, direction) {
                        selected = next;
                    }
                }
                Some(Action::Accept) => return Ok(Some(selected)),
                Some(Action::Cancel) => return Ok(None),
                None => {}
            }
        }
    }

    fn draw(
        &self,
        screen: &mut Screen,
        layout: &Layout,
        height: usize,
        selected: usize,
    ) -> Result<()> {
        let mut canvas = Canvas {
            out: &mut screen.out,
            layout,
            height,
        };
        queue!(canvas.out, terminal::Clear(terminal::ClearType::All))?;

        let fog = &self.theme.fog;
        for (index, line) in self.lines.iter().enumerate() {
            canvas.text(index, 0, &line.text, |i| {
                if line.is_shaded(i) {
                    fog.background
                } else {
                    fog.text
                }
            })?;
        }

        canvas.word(self.identifier, self.theme.identifier)?;

        // The selected candidate goes last so that overlapping candidates
        // don't paint over it.
        let styles = &self.theme.candidate;
        for (index, candidate) in self.candidates.iter().enumerate() {
            if index != selected {
                canvas.word(&candidate.word, styles.normal)?;
            }
        }
        canvas.word(&self.candidates[selected].word, styles.selected)?;

        canvas.out.flush()?;
        Ok(())
    }
}

enum Action {
    Move(Direction),
    Accept,
    Cancel,
}

fn action(key: KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    Some(match key.code {
        KeyCode::Up | KeyCode::Char('k') => Action::Move(Direction::Up),
        KeyCode::Down | KeyCode::Char('j') => Action::Move(Direction::Down),
        KeyCode::Left | KeyCode::Char('h') => Action::Move(Direction::Left),
        KeyCode::Right | KeyCode::Char('l') => Action::Move(Direction::Right),
        KeyCode::Enter => Action::Accept,
        KeyCode::Esc => Action::Cancel,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Action::Cancel,
        _ => return None,
    })
}

/// Draws text at positions of the pane lines.
struct Canvas<'a> {
    out: &'a mut Stdout,
    layout: &'a Layout,
    height: usize,
}

impl Canvas<'_> {
    fn word(&mut self, word: &Word, style: ContentStyle) -> io::Result<()> {
        self.text(word.line, word.column, &word.value, |_| style)
    }

    /// Draws `text` starting at character `column` of `line`, with the style
    /// of each character given by its index in the line.
    fn text(
        &mut self,
        line: usize,
        column: usize,
        text: &str,
        style_at: impl Fn(usize) -> ContentStyle,
    ) -> io::Result<()> {
        // Consecutive characters on one row with one style are printed
        // together to keep the output small.
        let mut run = String::new();
        let mut run_style = ContentStyle::new();
        let mut row = None;

        for (offset, ch) in text.chars().enumerate() {
            let index = column + offset;
            let cell = self.layout.cell(line, index);
            let style = style_at(index);

            if row != Some(cell.row) || style != run_style {
                self.flush(&mut run, run_style)?;
                if row != Some(cell.row) {
                    if cell.row >= self.height {
                        return Ok(());
                    }
                    queue!(self.out, cursor::MoveTo(to_u16(cell.col), to_u16(cell.row)))?;
                    row = Some(cell.row);
                }
                run_style = style;
            }
            run.push(ch);
        }
        self.flush(&mut run, run_style)
    }

    fn flush(&mut self, run: &mut String, style: ContentStyle) -> io::Result<()> {
        if !run.is_empty() {
            queue!(
                self.out,
                PrintStyledContent(StyledContent::new(style, run.as_str()))
            )?;
            run.clear();
        }
        Ok(())
    }
}

fn to_u16(value: usize) -> u16 {
    value.try_into().unwrap_or(u16::MAX)
}

/// Raw mode on the alternate screen, restored when dropped.
struct Screen {
    out: Stdout,
}

impl Screen {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut out = io::stdout();
        if let Err(err) = queue!(out, terminal::EnterAlternateScreen, cursor::Hide) {
            let _ = terminal::disable_raw_mode();
            return Err(err);
        }
        Ok(Self { out })
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = queue!(self.out, cursor::Show, terminal::LeaveAlternateScreen);
        let _ = self.out.flush();
        let _ = terminal::disable_raw_mode();
    }
}
