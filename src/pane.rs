//! Parsing of `tmux capture-pane -e` output into plain text lines.

/// One logical line of the pane (wrapped rows are already joined by tmux).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Line {
    /// Printable text with all escape sequences removed.
    pub text: String,
    /// Per character of `text`: whether it was drawn with a background colour
    /// or in reverse video, so the overlay can keep such regions visible.
    shaded: Vec<bool>,
}

impl Line {
    pub fn is_shaded(&self, char_index: usize) -> bool {
        self.shaded.get(char_index).copied().unwrap_or(false)
    }

    #[cfg(test)]
    fn plain(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            shaded: vec![false; text.chars().count()],
        }
    }
}

impl AsRef<str> for Line {
    fn as_ref(&self) -> &str {
        &self.text
    }
}

/// Splits captured pane output into lines. Trailing empty lines are dropped.
pub fn parse(captured: &str) -> Vec<Line> {
    captured
        .trim_end_matches('\n')
        .split('\n')
        .map(parse_line)
        .collect()
}

/// Graphic attributes that matter for the overlay.
#[derive(Debug, Default, Clone, Copy)]
struct Attributes {
    background: bool,
    reverse: bool,
}

impl Attributes {
    fn shaded(self) -> bool {
        self.background || self.reverse
    }

    /// Applies the parameters of an SGR (`ESC [ ... m`) sequence.
    fn apply_sgr(&mut self, params: &str) {
        // Sub-parameters (`38:5:1`) belong to the parameter before the colon.
        let mut codes = params.split(';').map(|p| {
            p.split(':')
                .next()
                .and_then(|code| code.parse::<u16>().ok())
                .unwrap_or(0)
        });

        while let Some(code) = codes.next() {
            match code {
                0 => *self = Self::default(),
                7 => self.reverse = true,
                27 => self.reverse = false,
                40..=47 | 100..=107 => self.background = true,
                49 => self.background = false,
                38 | 48 | 58 => {
                    // Extended colour: `5;n` or `2;r;g;b`.
                    let skip = match codes.next() {
                        Some(5) => 1,
                        Some(2) => 3,
                        _ => 0,
                    };
                    codes.by_ref().take(skip).for_each(drop);
                    if code == 48 {
                        self.background = true;
                    }
                }
                _ => {}
            }
        }
    }
}

fn parse_line(raw: &str) -> Line {
    let mut line = Line::default();
    let mut attributes = Attributes::default();
    // Shift Out / Shift In toggle the DEC special graphics character set.
    let mut line_drawing = false;
    let mut chars = raw.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => match chars.next() {
                // CSI: parameters, then a final byte in `@`..=`~`.
                Some('[') => {
                    let mut params = String::new();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            if c == 'm' {
                                attributes.apply_sgr(&params);
                            }
                            break;
                        }
                        params.push(c);
                    }
                }
                // OSC (e.g. hyperlinks): terminated by BEL or ESC \.
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\x07' {
                            break;
                        }
                        if c == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\x0e' => line_drawing = true,
            '\x0f' => line_drawing = false,
            _ => {
                let ch = if line_drawing { box_drawing(ch) } else { ch };
                line.text.push(ch);
                line.shaded.push(attributes.shaded());
            }
        }
    }

    line
}

/// Maps DEC special graphics characters to their Unicode box drawing forms.
fn box_drawing(ch: char) -> char {
    match ch {
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'q' => '─',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shading(line: &Line) -> String {
        (0..line.text.chars().count())
            .map(|i| if line.is_shaded(i) { '#' } else { '.' })
            .collect()
    }

    #[test]
    fn strips_escape_sequences() {
        let lines =
            parse("\x1b[1;32mgreen\x1b[0m plain\n\x1b]8;;http://x\x1b\\link\x1b]8;;\x07\n\n");
        assert_eq!(lines, vec![Line::plain("green plain"), Line::plain("link")]);
    }

    #[test]
    fn tracks_background_and_reverse() {
        let lines = parse("a\x1b[7mbc\x1b[27md\x1b[48;5;236me\x1b[49mf\x1b[38;5;41mg");
        assert_eq!(lines[0].text, "abcdefg");
        assert_eq!(shading(&lines[0]), ".##.#..");
    }

    #[test]
    fn converts_line_drawing() {
        let lines = parse("\x0elqk\x0f x");
        assert_eq!(lines[0].text, "┌─┐ x");
    }
}
