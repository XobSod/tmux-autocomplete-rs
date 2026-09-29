//! Drawing the picker over a dimmed copy of the window, and reading keys.

use std::io::{self, Stdout, Write};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{Attribute, ContentStyle, PrintStyledContent, StyledContent};
use crossterm::{cursor, queue, terminal};
use unicode_width::UnicodeWidthStr;

use crate::completion::Word;
use crate::layout::Cell;
use crate::picker::{Key, Mode, Outcome, Picker};
use crate::theme::Theme;
use crate::window::PaneView;

/// Runs the picker until the user decides.
pub fn run(picker: &mut Picker, theme: &Theme) -> Result<Outcome> {
    let mut screen = Screen::enter()?;

    loop {
        let (width, height) = terminal::size()?;
        draw(
            &mut screen.out,
            picker,
            theme,
            Cell {
                row: height.into(),
                col: width.into(),
            },
        )?;

        let Event::Key(key) = event::read()? else {
            // Redraw, e.g. after a resize.
            continue;
        };
        if let Some(key) = translate(key) {
            match picker.handle(key) {
                Outcome::Continue => {}
                outcome => return Ok(outcome),
            }
        }
    }
}

fn translate(key: KeyEvent) -> Option<Key> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    Some(match key.code {
        KeyCode::Char('c') if control => Key::Interrupt,
        KeyCode::Char('n') if control => Key::Tab,
        KeyCode::Char('p') if control => Key::BackTab,
        KeyCode::Char(_) if control => return None,
        KeyCode::Char(ch) => Key::Char(ch),
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Escape,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        _ => return None,
    })
}

fn draw(out: &mut Stdout, picker: &Picker, theme: &Theme, size: Cell) -> io::Result<()> {
    queue!(out, cursor::Hide, terminal::Clear(terminal::ClearType::All))?;
    let mut canvas = Canvas { out, size };

    for pane in &picker.panes {
        draw_pane(&mut canvas, pane, theme)?;
    }
    draw_borders(&mut canvas, &picker.panes, theme.fog.text)?;

    if let Some(identifier) = &picker.identifier {
        draw_word(&mut canvas, picker, identifier, theme.identifier)?;
    }
    // The selected candidate goes last so that overlapping candidates don't
    // paint over it.
    let selected = picker.selected();
    for &index in picker.matches() {
        if Some(index) != selected {
            draw_candidate(&mut canvas, picker, index, theme.candidate.normal)?;
        }
    }
    if let Some(index) = selected {
        draw_candidate(&mut canvas, picker, index, theme.candidate.selected)?;
    }

    for pane in &picker.panes {
        let back = pane.scrolled_back();
        if back > 0 {
            let label = format!("[{back}/{}]", pane.screen_top);
            let col = (pane.rect.right())
                .saturating_sub(label.width())
                .max(pane.rect.x);
            canvas.print(
                Cell {
                    row: pane.rect.y,
                    col,
                },
                &label,
                theme.prompt,
            )?;
        }
    }

    draw_prompt(&mut canvas, picker, theme)?;
    canvas.out.flush()
}

fn draw_pane(canvas: &mut Canvas, pane: &PaneView, theme: &Theme) -> io::Result<()> {
    let Some(first) = pane.layout.line_at_row(pane.scroll) else {
        return Ok(());
    };
    let last_row = pane.scroll + pane.rect.height;
    for line in
        (first..pane.layout.line_count()).take_while(|&l| pane.layout.first_row(l) < last_row)
    {
        let text = &pane.lines[line];
        canvas.text(pane, line, 0, text.text.chars().count(), |index| {
            if text.is_shaded(index) {
                theme.fog.background
            } else {
                theme.fog.text
            }
        })?;
    }
    Ok(())
}

/// Draws lines in the gaps between panes.
fn draw_borders(canvas: &mut Canvas, panes: &[PaneView], style: ContentStyle) -> io::Result<()> {
    if panes.len() < 2 {
        return Ok(());
    }
    let width = panes.iter().map(|p| p.rect.right()).max().unwrap_or(0);
    let height = panes.iter().map(|p| p.rect.bottom()).max().unwrap_or(0);

    for row in 0..height {
        for col in 0..width {
            let cell = Cell { row, col };
            if panes.iter().any(|p| p.rect.contains(cell)) {
                continue;
            }
            let beside = |p: &PaneView| {
                (p.rect.y..p.rect.bottom()).contains(&row)
                    && (p.rect.right() == col || col + 1 == p.rect.x)
            };
            let above_or_below = |p: &PaneView| {
                (p.rect.x..p.rect.right()).contains(&col)
                    && (p.rect.bottom() == row || row + 1 == p.rect.y)
            };
            let symbol = match (panes.iter().any(beside), panes.iter().any(above_or_below)) {
                (true, true) => "┼",
                (true, false) => "│",
                (false, true) => "─",
                (false, false) => "┼",
            };
            canvas.print(cell, symbol, style)?;
        }
    }
    Ok(())
}

/// Draws a candidate if it is in view, underlining the characters matched
/// by the filter.
fn draw_candidate(
    canvas: &mut Canvas,
    picker: &Picker,
    index: usize,
    style: ContentStyle,
) -> io::Result<()> {
    let word = &picker.candidates[index].word;
    let pane = &picker.panes[word.pane];
    let first = pane.cell(word.line, word.column).row;
    let last = pane.cell(word.line, word.column + word.len()).row;
    if !(first..=last).any(|row| pane.is_in_view(row)) {
        return Ok(());
    }

    let matched = picker.matched_chars(index);
    let mut underlined = style;
    underlined.attributes.set(Attribute::Underlined);
    canvas.text(pane, word.line, word.column, word.len(), |i| {
        if matched.binary_search(&(i - word.column)).is_ok() {
            underlined
        } else {
            style
        }
    })
}

fn draw_word(
    canvas: &mut Canvas,
    picker: &Picker,
    word: &Word,
    style: ContentStyle,
) -> io::Result<()> {
    canvas.text(
        &picker.panes[word.pane],
        word.line,
        word.column,
        word.len(),
        |_| style,
    )
}

fn draw_prompt(canvas: &mut Canvas, picker: &Picker, theme: &Theme) -> io::Result<()> {
    if picker.mode == Mode::Normal && picker.filter.is_empty() {
        return Ok(());
    }
    let status = match picker.selected() {
        Some(selected) => {
            let position = picker.position_of(selected).map_or(0, |p| p + 1);
            format!("[{position}/{}]", picker.matches().len())
        }
        None => "[no match]".to_owned(),
    };
    let row = canvas.size.row.saturating_sub(1);
    let prompt = format!("/{}", picker.filter);
    let line = format!("{prompt}  {status}");
    let padding = canvas.size.col.saturating_sub(line.width());
    canvas.print(
        Cell { row, col: 0 },
        &format!("{line}{}", " ".repeat(padding)),
        theme.prompt,
    )?;

    if picker.mode == Mode::Filter {
        let col = prompt.width().min(canvas.size.col.saturating_sub(1));
        queue!(
            canvas.out,
            cursor::MoveTo(to_u16(col), to_u16(row)),
            cursor::Show
        )?;
    }
    Ok(())
}

struct Canvas<'a> {
    out: &'a mut Stdout,
    size: Cell,
}

impl Canvas<'_> {
    fn print(&mut self, at: Cell, text: &str, style: ContentStyle) -> io::Result<()> {
        if at.row >= self.size.row || at.col >= self.size.col {
            return Ok(());
        }
        queue!(
            self.out,
            cursor::MoveTo(to_u16(at.col), to_u16(at.row)),
            PrintStyledContent(StyledContent::new(style, text))
        )
    }

    /// Draws `count` characters of `line` of `pane` starting at character
    /// `column`, with the style of each given by its index in the line.
    /// Only rows in view are drawn.
    fn text(
        &mut self,
        pane: &PaneView,
        line: usize,
        column: usize,
        count: usize,
        style_at: impl Fn(usize) -> ContentStyle,
    ) -> io::Result<()> {
        // Consecutive characters on one row with one style are printed
        // together to keep the output small.
        let mut run = String::new();
        let mut run_style = ContentStyle::new();
        let mut run_row = None;

        let chars = pane
            .layout
            .chars(line, &pane.lines[line].text)
            .enumerate()
            .skip(column)
            .take(count);
        for (index, (ch, cell)) in chars {
            let Some(at) = pane.to_window(cell) else {
                self.flush(&mut run, run_style)?;
                run_row = None;
                continue;
            };
            let style = style_at(index);
            if run_row != Some(at.row) || style != run_style {
                self.flush(&mut run, run_style)?;
                if run_row != Some(at.row) {
                    if at.row >= self.size.row || at.col >= self.size.col {
                        continue;
                    }
                    queue!(self.out, cursor::MoveTo(to_u16(at.col), to_u16(at.row)))?;
                    run_row = Some(at.row);
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
