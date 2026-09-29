# tmux-autocomplete

Complete the word before the cursor from anything in the panes of the current
tmux window, including their scrollback. Inspired by [reconquest/tmux-autocomplete][upstream],
written in Rust, because of course, compatible with its options, themes
and key bindings.

[upstream]: https://github.com/reconquest/tmux-autocomplete

## Install

```sh
cargo install --path .
# optional: system-wide themes and the URL opener helper
sudo install -Dm644 share/themes/*.theme -t /usr/share/tmux-autocomplete/themes/
sudo install -Dm755 share/tmux-autocomplete-url /usr/local/bin/
```

The `light` and `dark` themes are built in, so installing theme files is only
needed to customise them. The built-in themes follow the original project's
colours.

## Usage

Add to `~/.tmux.conf`:

```
bind-key C-Space run -b 'tmux-autocomplete --theme=dark'
```

Press the binding after typing the start of a word. A popup over the window
highlights every candidate starting with it, in all panes and their
scrollback. When there is only one candidate it is inserted right away.

| Key | Action |
| --- | --- |
| `h` `j` `k` `l`, arrows | Move. Stays within the pane, scrolling through its history, then continues in the neighbouring pane. |
| `/` | Fuzzy filter: type to narrow the candidates, like fzf (`hw25` finds `history_word_25`; smart case). Matched characters are underlined and the best match is selected. fzf syntax works: `'exact`, `^prefix`, `suffix$`, `!exclude`. `Enter` inserts, `Esc` returns to moving with the filter kept, `Backspace` on an empty filter clears it. Arrows, `Tab`/`Ctrl-n` and `Shift-Tab`/`Ctrl-p` move while filtering. |
| `n` `N` | Next / previous candidate (best match first while filtering) |
| `Enter` | Insert the selected candidate (or run `--exec` on it) |
| `y` | Copy it to a tmux buffer and the system clipboard (via `set-clipboard`) |
| `Esc` `q` `Ctrl-C` | Cancel |

`tmux-autocomplete --help` lists the options: custom regexps for the
identifier (`-c`) and candidates (`-r`), completion without a prefix (`-n`),
running a program on the selection instead of pasting it (`-e`, see
`share/tmux-autocomplete-url`), how much scrollback to search (`--history`),
`--window` to use a new window instead of a popup (tmux before 3.3), themes and
debug logging.

## Themes

Themes are looked up as `NAME.theme` in `~/.config/tmux-autocomplete/themes/`,
then `/usr/share/tmux-autocomplete/themes/` (override with `--theme-path`).

```yaml
identifier: default+ub:default   # the prefix being completed
candidate:
    normal: green:default
    selected: 16+b:green
fog:                             # the dimmed pane behind the candidates
    text: 236:default
    background: 238:236          # text that had a background colour
prompt: default+i:default        # optional: filter prompt, scroll position
```

Styles are `FG[+ATTRS][:BG[+h]]`: colour names (`black` … `white`,
`default`) or 256-colour numbers, attributes `b` bold, `u` underline, `i`
inverse, `B` blink, `s` strikethrough, `h` bright.

## Development

```sh
cargo test                     # unit and end-to-end tests
scripts/demo.sh k k Enter      # watch a completion; or: scripts/demo.sh / 2 5 Enter
```

The end-to-end tests (`tests/e2e/`) need tmux 3.3 or later. Each one starts
its own pair of tmux servers with sockets in a temporary directory: an outer
server shows a client attached to an inner one, so the popup can be captured
and typed into. A test theme with distinctive colours lets the tests read
which candidates are highlighted, selected or underlined. Tests wait for
screen states rather than sleeping, and run in parallel in well under a
second.

`scripts/demo.sh` uses the same idea from the shell (helpers in
`scripts/nested-rig.sh`) and prints the popup and the resulting prompt line.

## Differences from the Go version

- Works with `default-terminal tmux-256color`.
- Correct cursor mapping in split panes narrower than the window, and for
  wide (CJK, emoji) characters.
- Arguments are passed to the worker with proper quoting, so regexps
  containing `$`, `#`, quotes or backticks stay intact.
- User themes take precedence over system themes of the same name.
- Invalid theme colours are reported instead of silently becoming black.
- `Esc` cancels; the terminal is redrawn on resize.
- Pastes through a dedicated tmux buffer, leaving your buffers untouched.
- New: popup overlay, all panes of the window, scrollback, filtering, copying.
