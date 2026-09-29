#!/bin/bash
# Nested test rig: outer server "tac-outer" shows a client attached to inner server "tac-inner".
# Source it in bash, then: ti <args> (inner tmux), to <args> (outer tmux), screen (what the client
# shows), keys <key names> (type into the client).
IN_CONF=/tmp/tac-inner.conf OUT_CONF=/tmp/tac-outer.conf
ti() { tmux -L tac-inner -f $IN_CONF "$@"; }
to() { tmux -L tac-outer -f $OUT_CONF "$@"; }
screen() { to capture-pane -p ${1:-} -t o:0; }
keys() { for k in "$@"; do to send-keys -t o:0 "$k"; sleep 0.15; done; }
rig_start() { # $1 = width, $2 = height of the client terminal (status line included)
  printf 'set -g default-terminal tmux-256color\nset -g status-position top\nset -g history-limit 5000\n%s\n' "${INNER_EXTRA:-}" > $IN_CONF
  printf 'set -g status off\nset -g default-terminal tmux-256color\n' > $OUT_CONF
  ti kill-server 2>/dev/null; to kill-server 2>/dev/null
  ti new-session -d -s t -x $1 -y $(($2 - 1)) "env -i HOME=$HOME PATH=/usr/bin:/bin TERM=tmux-256color PS1='\$ ' bash --norc --noprofile"
  to new-session -d -s o -x $1 -y $2 "env -u TMUX TERM=tmux-256color tmux -L tac-inner attach -t t"
  sleep 0.8
}
rig_stop() { # tmux leaves its socket files behind, so remove them too
  local sockets="$(to display -p '#{socket_path}' 2>/dev/null) $(ti display -p '#{socket_path}' 2>/dev/null)"
  to kill-server 2>/dev/null; ti kill-server 2>/dev/null
  rm -f $sockets
}
