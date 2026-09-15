---
description: Put the colai toolbar on screen
allowed-tools: Bash(colai-toolbar:*)
disable-model-invocation: true
---

Start the colai toolbar, then tell the user in one short line that it is up and that
Ctrl+Alt+Space brings it back if they put it away.

Run exactly this, and nothing else:

```
colai-toolbar show --pidfile "$HOME/.config/ai.colai.toolbar/colai-toolbar.pid"
```

Two things about that command:

- It returns immediately. The toolbar puts itself into a process group of its own and
  keeps running after this session ends — it is a desktop overlay, not a subprocess of this
  conversation. Do not add `setsid` or `&`; it does this itself, and the sandbox refuses
  `setsid` anyway.
- If it prints that there is no screen to draw on, say so plainly and stop. It needs X11;
  it refuses Wayland deliberately rather than pointing at the wrong window.

Do not offer to do anything else afterwards. The toolbar is the interface now.
