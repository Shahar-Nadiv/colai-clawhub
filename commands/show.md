---
description: Put the colai toolbar on screen
allowed-tools: Bash(colai-toolbar:*)
disable-model-invocation: true
---

Start the colai toolbar, then tell the user in one short line that it is up, that it is
pointed at this conversation, and that Ctrl+Alt+Space shows and hides it.

Run exactly this, and nothing else:

```
colai-toolbar show --pidfile "$HOME/.config/ai.colai.toolbar/colai-toolbar.pid" --in "${CLAUDE_SESSION_ID}"
```

Three things about that command:

- `--in` is this conversation. The toolbar opens pointed at it, so the first thing marked
  on screen already has somewhere to go and nobody has to pick from a list of every chat on
  the machine. Running this from a different conversation later moves the toolbar to that
  one. Claude Code substitutes `${CLAUDE_SESSION_ID}` before the command runs; if it ever
  arrives unsubstituted the toolbar ignores it and opens on a choice, so leave it as it is.

- It returns immediately. The toolbar puts itself into a process group of its own and
  keeps running after this session ends — it is a desktop overlay, not a subprocess of this
  conversation. Do not add `setsid` or `&`; it does this itself, and the sandbox refuses
  `setsid` anyway.

- If it prints that there is no screen to draw on, say so plainly and stop. It needs X11;
  it refuses Wayland deliberately rather than pointing at the wrong window.

Do not offer to do anything else afterwards. The toolbar is the interface now.
