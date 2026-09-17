#!/bin/sh
# Hand this session anything the toolbar left for it.
#
# The toolbar cannot speak into a conversation somebody is sitting in — it runs its own
# `claude`, and pointing that at a live chat puts two processes on one transcript. So a mark
# made for this conversation is written to disk instead, and this runs inside the session,
# on the person's next message, and reads it out.
#
# `UserPromptSubmit` and not `SessionStart`, because the mark is usually made long after the
# session began — somebody has the toolbar open beside a chat that has been running for an
# hour. The cost is that the mark waits for their next Enter.
#
# stdout here goes to the MODEL, which is exactly what is wanted: this is context arriving,
# not a notice for the person. (`systemMessage` is the other channel and reaches them; see
# say-colai-is-here.sh, which wants the opposite.)

set -eu

# Which conversation this is. Claude Code sets it for every hook; without it there is no
# way to tell which marks are ours and the honest thing is to hand over nothing.
[ -n "${CLAUDE_CODE_SESSION_ID:-}" ] || exit 0

WAITING="${XDG_CONFIG_HOME:-$HOME/.config}/ai.colai.toolbar/outbox/$CLAUDE_CODE_SESSION_ID"
[ -d "$WAITING" ] || exit 0

# Oldest first, so two marks arrive in the order they were made.
#
# `.part` directories are skipped: the toolbar builds each mark under that name and renames
# it into place when it is whole, so anything still wearing it is half written. A picture
# with no question is worse than a mark that waits for the next message.
handed=0
for mark in $(ls -1 "$WAITING" 2>/dev/null | sort); do
  at="$WAITING/$mark"
  case "$mark" in *.part) continue ;; esac
  [ -f "$at/say.txt" ] || continue

  cat "$at/say.txt"
  printf '\n'
  handed=$((handed + 1))

  # Gone once handed over. A mark that survived would arrive again on the next message and
  # go on arriving — and this is context the model acts on, so repeating it means acting on
  # it twice. Removed after printing rather than before, so a failure here costs a repeat
  # rather than a mark nobody ever saw.
  rm -rf "$at"
done

# Nothing waiting is the common case and says nothing at all: a hook that announced its own
# emptiness on every message would put a line of noise in front of every single turn.
[ "$handed" -gt 0 ] || exit 0
