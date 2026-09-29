//! Theme files and the style syntax they use.
//!
//! A style is `FOREGROUND[+ATTRIBUTES][:BACKGROUND[+h]]`, where colours are
//! names (`black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`,
//! `white`, `default`) or 256-colour palette numbers. Attributes are `b`
//! (bold), `B` (blink), `u` (underline), `i` (inverse), `s` (strikethrough)
//! and `h` (high intensity), e.g. `16+b:green` or `default+ub:default`.

use std::path::PathBuf;
use std::{env, fs, io};

use anyhow::{Context, Result, anyhow, bail};
use crossterm::style::{Attribute, Color, ContentStyle};
use serde::Deserialize;

pub const SYSTEM_THEME_DIR: &str = "/usr/share/tmux-autocomplete/themes/";
pub const USER_THEME_DIR: &str = "~/.config/tmux-autocomplete/themes/";

const DEFAULT_PROMPT_STYLE: &str = "default+i:default";

const BUILTIN_THEMES: &[(&str, &str)] = &[
    ("light", include_str!("../share/themes/light.theme")),
    ("dark", include_str!("../share/themes/dark.theme")),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// The identifier being completed.
    pub identifier: ContentStyle,
    pub candidate: CandidateStyles,
    /// The dimmed pane contents behind the candidates.
    pub fog: FogStyles,
    /// The filter prompt and scroll position indicators.
    pub prompt: ContentStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CandidateStyles {
    pub normal: ContentStyle,
    pub selected: ContentStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FogStyles {
    pub text: ContentStyle,
    /// Text that had a background colour or was in reverse video.
    pub background: ContentStyle,
}

#[derive(Deserialize)]
struct ThemeFile {
    identifier: String,
    candidate: CandidateFile,
    fog: FogFile,
    /// Optional, for compatibility with themes of the Go version.
    prompt: Option<String>,
}

#[derive(Deserialize)]
struct CandidateFile {
    normal: String,
    selected: String,
}

#[derive(Deserialize)]
struct FogFile {
    text: String,
    background: String,
}

impl Theme {
    /// Loads theme `name` from the first directory in `dirs` that has it,
    /// falling back to the built-in themes.
    pub fn load(dirs: &[PathBuf], name: &str) -> Result<Self> {
        for dir in dirs {
            let path = dir.join(format!("{name}.theme"));
            match fs::read_to_string(&path) {
                Ok(source) => {
                    return Self::parse(&source)
                        .with_context(|| format!("invalid theme file {}", path.display()));
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => {
                    return Err(err)
                        .with_context(|| format!("unable to read theme file {}", path.display()));
                }
            }
        }

        let (_, source) = BUILTIN_THEMES
            .iter()
            .find(|(builtin, _)| *builtin == name)
            .ok_or_else(|| anyhow!("no theme named {name:?} found"))?;
        Self::parse(source).context("invalid built-in theme")
    }

    pub fn parse(source: &str) -> Result<Self> {
        let file: ThemeFile = serde_yaml_ng::from_str(source)?;
        Ok(Self {
            identifier: parse_style(&file.identifier).context("identifier")?,
            candidate: CandidateStyles {
                normal: parse_style(&file.candidate.normal).context("candidate.normal")?,
                selected: parse_style(&file.candidate.selected).context("candidate.selected")?,
            },
            fog: FogStyles {
                text: parse_style(&file.fog.text).context("fog.text")?,
                background: parse_style(&file.fog.background).context("fog.background")?,
            },
            prompt: parse_style(file.prompt.as_deref().unwrap_or(DEFAULT_PROMPT_STYLE))
                .context("prompt")?,
        })
    }
}

/// Splits a `:`-separated list of theme directories, expanding `~/`.
pub fn theme_dirs(spec: Option<&str>) -> Result<Vec<PathBuf>> {
    let default = format!("{USER_THEME_DIR}:{SYSTEM_THEME_DIR}");
    spec.unwrap_or(&default)
        .split(':')
        .map(|dir| {
            if dir.is_empty() {
                bail!("empty directory in theme path");
            }
            Ok(match dir.strip_prefix("~/") {
                Some(rest) => home_dir()?.join(rest),
                None => PathBuf::from(dir),
            })
        })
        .collect()
}

fn home_dir() -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

pub fn parse_style(spec: &str) -> Result<ContentStyle> {
    let mut style = ContentStyle::new();
    if spec.is_empty() || spec == "off" || spec == "reset" {
        return Ok(style);
    }

    let (foreground, background) = match spec.split_once(':') {
        Some((fg, bg)) => (fg, Some(bg)),
        None => (spec, None),
    };

    let (color, flags) = split_flags(foreground);
    let mut bright = false;
    for flag in flags.chars() {
        let attribute = match flag {
            'b' => Attribute::Bold,
            'B' => Attribute::SlowBlink,
            'u' => Attribute::Underlined,
            'i' => Attribute::Reverse,
            's' => Attribute::CrossedOut,
            'h' => {
                bright = true;
                continue;
            }
            other => bail!("unknown attribute {other:?} in style {spec:?}"),
        };
        style.attributes.set(attribute);
    }
    style.foreground_color = parse_color(color, bright)?;

    if let Some(background) = background {
        let (color, flags) = split_flags(background);
        if let Some(other) = flags.chars().find(|&f| f != 'h') {
            bail!("unknown background attribute {other:?} in style {spec:?}");
        }
        style.background_color = parse_color(color, flags.contains('h'))?;
    }

    Ok(style)
}

fn split_flags(part: &str) -> (&str, &str) {
    part.split_once('+').unwrap_or((part, ""))
}

fn parse_color(name: &str, bright: bool) -> Result<Option<Color>> {
    if name.is_empty() {
        return Ok(None);
    }
    if let Ok(index) = name.parse::<u8>() {
        return Ok(Some(Color::AnsiValue(index)));
    }

    let (normal, high) = match name {
        "black" => (Color::Black, Color::DarkGrey),
        "red" => (Color::DarkRed, Color::Red),
        "green" => (Color::DarkGreen, Color::Green),
        "yellow" => (Color::DarkYellow, Color::Yellow),
        "blue" => (Color::DarkBlue, Color::Blue),
        "magenta" => (Color::DarkMagenta, Color::Magenta),
        "cyan" => (Color::DarkCyan, Color::Cyan),
        "white" => (Color::Grey, Color::White),
        "default" => (Color::Reset, Color::Reset),
        other => bail!("unknown colour {other:?}"),
    };
    Ok(Some(if bright { high } else { normal }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_styles() {
        let style = parse_style("16+b:green").unwrap();
        assert_eq!(style.foreground_color, Some(Color::AnsiValue(16)));
        assert_eq!(style.background_color, Some(Color::DarkGreen));
        assert!(style.attributes.has(Attribute::Bold));

        let style = parse_style("default+ub:default").unwrap();
        assert_eq!(style.foreground_color, Some(Color::Reset));
        assert!(style.attributes.has(Attribute::Underlined));

        let style = parse_style("red+h:blue+h").unwrap();
        assert_eq!(style.foreground_color, Some(Color::Red));
        assert_eq!(style.background_color, Some(Color::Blue));
    }

    #[test]
    fn rejects_unknown_colours() {
        assert!(parse_style("purple").is_err());
        assert!(parse_style("red+x").is_err());
    }

    #[test]
    fn builtin_themes_parse() {
        for (name, _) in BUILTIN_THEMES {
            Theme::load(&[], name).unwrap();
        }
        assert!(Theme::load(&[], "missing").is_err());
    }

    #[test]
    fn theme_dirs_expand_home() {
        let dirs = theme_dirs(Some("/a:~/b")).unwrap();
        assert_eq!(dirs[0], PathBuf::from("/a"));
        assert!(dirs[1].ends_with("b") && dirs[1].is_absolute());
        assert!(theme_dirs(Some("/a::/b")).is_err());
    }
}
