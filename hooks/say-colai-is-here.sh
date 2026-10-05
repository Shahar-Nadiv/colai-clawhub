#!/bin/sh
# Start colai when a session begins, and say one line about it.
#
# colai is a desktop overlay, not a chat command. A plugin can only start something in one of
# two ways — a slash command or a hook — and summoning a GUI by typing a command into a chat is
# the wrong shape for it. So the toolbar starts itself here, from the SessionStart hook: it is
# simply there when Claude Code opens, and <KEY> shows and hides it. Nothing to type.
#
# The toolbar is a borderless always-on-top window with no taskbar entry, and its shortcut is
# grabbed at the display server rather than read from the terminal — so it already works while
# you are typing in Claude Code.
#
# Claude Code reads two channels from a hook and they are not the same: stdout goes to the
# MODEL, `systemMessage` goes to the PERSON. Printing the notice plainly would tell Claude
# about colai and show you nothing.

set -eu

# The key, as the toolbar itself resolves it — see hotkey.rs. Ctrl+Space is deliberately not
# the default: on any machine with an input-method switcher, which is most, the desktop takes
# it before an application sees it.
KEY="${COLAI_HOTKEY:-Ctrl+Alt+Space}"

# Where the running toolbar records its pid. Derived exactly as the launcher derives it, and a
# test holds the two to the same string — see plugin.test.ts "the hook looks in that exact place".
PIDFILE="${XDG_CONFIG_HOME:-$HOME/.config}/ai.colai.toolbar/colai-toolbar.pid"

# Whether a process with this pid is alive.
#
# On Linux and macOS that is `kill -0`. On Windows it is not, and the difference cost every
# session start a launch nobody asked for: the toolbar writes its NATIVE Windows pid (see
# whereabouts.rs, `std::process::id()`), while Git Bash's `kill` speaks MSYS pids, a separate
# numbering. `kill -0 16420` answered "no such process" about a toolbar sitting right there,
# so the hook ran the launcher, which showed the toolbar and said it was starting.
#
# `ps -W` lists native processes too, with the native pid in the WINPID column — the fourth,
# or the fifth when MSYS prefixes a row with a one-letter state (S, I, O).
alive() {
  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
      ps -W 2>/dev/null | awk -v p="$1" '
        { w = ($1 ~ /^[0-9]+$/) ? $4 : $5 }
        w == p { found = 1 }
        END { exit !found }'
      ;;
    *) kill -0 "$1" 2>/dev/null ;;
  esac
}

running=no
if [ -r "$PIDFILE" ]; then
  pid=$(cat "$PIDFILE" 2>/dev/null || true)
  # A pidfile outlives a crash, so the pid is checked rather than believed.
  if [ -n "$pid" ] && alive "$pid"; then
    running=yes
  fi
fi

# The launcher beside this hook, which resolves the right binary for the platform, unpacks and
# verifies it, and runs it. `$CLAUDE_PLUGIN_ROOT` is set for hooks; fall back to this file's dir.
ROOT="${CLAUDE_PLUGIN_ROOT:-$(CDPATH= cd "$(dirname "$0")/.." 2>/dev/null && pwd)}"
LAUNCHER="$ROOT/bin/colai-toolbar"

# Which conversation this is, so the toolbar opens pointed at it.
#
# Claude Code hands a hook its session as JSON on stdin — not in the environment, which is
# where the toolbar used to read it when a slash command started it from the Bash tool. Without
# this the rail opened on an empty receiver and asked a question the launch could have answered.
# Read without a JSON parser (nothing may need a runtime here), and kept only if it looks like an
# id: letters, digits and dashes, so nothing the input says can reach the command line as more.
session=
if [ ! -t 0 ]; then
  session=$(cat | tr -d '\r\n' | sed -n 's/.*"session_id"[[:space:]]*:[[:space:]]*"\([A-Za-z0-9-]*\)".*/\1/p')
fi

if [ "$running" = yes ]; then
  title="Colai is running"
  said="Press $KEY to show or hide the toolbar, then point at anything on screen and send it here."
elif [ "${COLAI_AUTOSTART:-1}" = "0" ]; then
  # An explicit opt-out for anyone who would rather it not start itself.
  title="Colai is installed, but autostart is off"
  said="COLAI_AUTOSTART=0 is set. Run colai-toolbar show to start it; then $KEY shows and hides it."
elif [ -f "$LAUNCHER" ]; then
  # Ask first, start second. The launch below is detached and silent, so a toolbar that cannot
  # start here — no build for this machine, a Wayland session, a missing library — would vanish
  # without a word while this line promised it was starting. `check` asks the launcher's own
  # questions without running anything and says what is wrong; only `ok` earns a launch.
  # (The first check also unpacks and verifies the binary, so the launch that follows is quick.)
  if why=$(sh "$LAUNCHER" check 2>&1 >/dev/null); then
    # Detached, so this hook returns at once (it has a few-second budget) and the toolbar
    # outlives the shell that launched it. The launcher self-derives its pidfile from `show`.
    # `--in` names the conversation (see session.rs `came_from`); only on a fresh start, so a
    # second session opening does not pull a toolbar somebody already pointed elsewhere.
    if [ -n "$session" ]; then
      nohup sh "$LAUNCHER" show --in "$session" >/dev/null 2>&1 &
    else
      nohup sh "$LAUNCHER" show >/dev/null 2>&1 &
    fi
    title="Colai is starting"
    said="Press $KEY to show or hide the toolbar, then point at anything on screen and send it to this session."
  else
    title="Colai can't start on this machine yet"
    said=$why
  fi
else
  title="Colai could not find its launcher"
  said="Reinstall it with: claude plugin install colai@colai"
fi

# JSON by hand, because this must not need a runtime installed to say one sentence. The reason
# above is the launcher's own words, so it is made safe here: backslashes and quotes escaped,
# control characters and line breaks folded into spaces, and each part kept short.
#
# Two lines on purpose. Claude Code prints this under its own "SessionStart says:" prefix, and a
# single sentence there was easy to read straight past — so the headline stands alone, with the
# jellyfish in front of it, and what to do comes on the line beneath. A hook cannot colour its
# message; an emoji and a line break are what there is.
safe() {
  printf '%s' "$1" | tr '\r\n\t' '   ' | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/  */ /g' | cut -c1-600
}
printf '{"systemMessage":"🪼 %s\\n   %s"}\n' "$(safe "$title")" "$(safe "$said")"
