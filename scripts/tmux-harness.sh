#!/bin/bash
# Usage: [TERMTYPE=...] tac-harness.sh BIN TYPED [KEYS...]
# Drives one completion in an isolated tmux server and prints the resulting prompt line.
set -u
BIN=$1; TYPED=$2; shift 2
CONF=$(mktemp); echo "set -g default-terminal ${TERMTYPE:-tmux-256color}" > "$CONF"
t() { tmux -L tac-test -f "$CONF" "$@"; }
t kill-server 2>/dev/null
t new-session -d -s t -x 100 -y 20 "env -i HOME=$HOME PATH=/usr/local/bin:/usr/bin:/bin PS1='$ ' bash --norc --noprofile"
SOCKPATH=$(t display -p '#{socket_path}')
sleep 0.4
t send-keys -t t:0 'echo hello_world helium_balloon "(1.2.3.4)" ∞日本 hexagon' Enter
sleep 0.2
t send-keys -t t:0 "$TYPED"
sleep 0.2
t run-shell -b -t t:0 "TMUX=$SOCKPATH,0,0 $BIN ${EXTRA:-}"
sleep 0.8
if [ -n "${SHOW:-}" ]; then t list-windows -t t -F '#{window_index} #{pane_current_command}'; fi
for k in "$@"; do t send-keys -t t:1 "$k"; sleep 0.15; done
sleep 0.4
t capture-pane -p -t t:0 | grep -v '^$' | tail -1
t kill-server; rm -f "$CONF"
