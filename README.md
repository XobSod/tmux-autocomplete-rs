# tmux-autocomplete

Complete the word before the cursor from anything visible in the current tmux
pane. A Rust reimplementation of [reconquest/tmux-autocomplete][upstream],
compatible with its options, themes and key bindings.

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

Press the binding after typing the start of a word. Candidates starting with
it are highlighted; move with the arrow keys or `h` `j` `k` `l`, press `Enter`
to insert the selected one, `Esc` or `Ctrl-C` to cancel. When there is only
one candidate it is inserted right away.

`tmux-autocomplete --help` lists the options: custom regexps for the
identifier (`-c`) and candidates (`-r`), completion without a prefix (`-n`),
running a program on the selection instead of pasting it (`-e`, see
`share/tmux-autocomplete-url`), themes and debug logging.

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
```

Styles are `FG[+ATTRS][:BG[+h]]`: colour names (`black` … `white`,
`default`) or 256-colour numbers, attributes `b` bold, `u` underline, `i`
inverse, `B` blink, `s` strikethrough, `h` bright.

## Development

```sh
cargo test
scripts/tmux-harness.sh target/debug/tmux-autocomplete 'echo he' Left Enter
```

The harness drives a completion in an isolated tmux server (`-L tac-test`) and
prints the resulting prompt line.

## Differences from the Go version

- Works with `default-terminal tmux-256color`.
- Correct cursor mapping in split panes narrower than the window, and for
  wide (CJK, emoji) characters.
- The worker is started without a shell, so regexps containing `$`, quotes or
  backticks are passed through intact.
- User themes take precedence over system themes of the same name.
- Invalid theme colours are reported instead of silently becoming black.
- `Esc` cancels; the terminal is redrawn on resize.
- Pastes through a dedicated tmux buffer, leaving your buffers untouched.
