#!/bin/sh
# One line at the top of a session, saying colai is here and how to reach it.
#
# The toolbar is a borderless always-on-top window with no taskbar entry, and its shortcut is
# grabbed at the display server rather than read from the terminal — so it already works while
# you are typing in Claude Code, and has from the beginning. Nothing ever said so, which is the
# only reason it went unused.
#
# Claude Code reads two channels from a hook and they are not the same: stdout goes to the
# MODEL, `systemMessage` goes to the PERSON. Printing the notice plainly would tell Claude
# about colai and show you nothing.

set -eu

# The key, as the toolbar itself resolves it — see hotkey.rs. Ctrl+Space is deliberately not
# the default: on any machine with an input-method switcher, which is most, the desktop takes
# it before an application sees it.
KEY="${COLAI_HOTKEY:-Ctrl+Alt+Space}"

PIDFILE="${XDG_CONFIG_HOME:-$HOME/.config}/ai.colai.toolbar/colai-toolbar.pid"
running=no
if [ -r "$PIDFILE" ]; then
  pid=$(cat "$PIDFILE" 2>/dev/null || true)
  # A pidfile outlives a crash, so the pid is checked rather than believed.
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
    running=yes
  fi
fi

if [ "$running" = yes ]; then
  said="colai is running — $KEY shows the toolbar, and hides it again."
else
  # Saying a key works when nothing is listening for it is worse than saying nothing.
  said="colai is installed — run /colai:show to start the toolbar, then $KEY shows and hides it."
fi

# JSON by hand, because this must not need a runtime installed to say one sentence. The only
# character that can appear here and break it is a quote, and none of the text above has one.
printf '{"systemMessage":"%s"}\n' "$said"
