#!/bin/bash
# Drives one completion in an isolated nested tmux (see nested-rig.sh) and
# prints the popup and the resulting prompt line.
#
# Usage: scripts/demo.sh [KEYS...]        e.g. scripts/demo.sh k k Enter
# Environment: TYPED (text typed at the prompt, default "echo hi"),
#              EXTRA (extra tmux-autocomplete options), BIN (binary to test).
set -u
cd "$(dirname "$0")/.."
source scripts/nested-rig.sh
BIN=${BIN:-$PWD/target/debug/tmux-autocomplete}

rig_start 80 12
ti send-keys -t t 'for i in $(seq 1 40); do echo "line $i history_word_$i"; done; echo "visible_left hi_there"' Enter
sleep 0.4
ti split-window -h -t t "env -i HOME=$HOME PATH=/usr/bin:/bin TERM=tmux-256color PS1='\$ ' bash --norc --noprofile"
sleep 0.4
ti send-keys -t t:0.1 'echo right_pane_word hierarchy.txt' Enter
sleep 0.2
ti select-pane -t t:0.0
ti send-keys -t t:0.0 "${TYPED:-echo hi}"
sleep 0.3
ti run-shell -b -t t:0.0 "$BIN ${EXTRA:-}"
sleep 1

# Candidates are shown as [text], the selected one as {text} (light theme).
decode() {
  sed -E 's/\x1b\[1m\x1b\[38;5;230m\x1b\[48;5;232m([^\x1b]*)/{\1}/g; s/\x1b\[38;5;232m([^\x1b]*)/[\1]/g; s/\x1b\[[0-9;]*m//g'
}
echo "===== popup"
screen -e | decode
for k in "$@"; do keys "$k"; done
sleep 0.5
echo "===== prompt line after: $*"
ti capture-pane -p -t t:0.0 | grep -v '^$' | tail -1
rig_stop
