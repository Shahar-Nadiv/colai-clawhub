---
description: Stop the colai toolbar
allowed-tools: Bash(colai-toolbar:*)
disable-model-invocation: true
---

Stop the colai toolbar, then say in one short line that it has gone and that
`/colai:show` brings it back.

Run exactly this, and nothing else:

```
colai-toolbar quit
```

Nothing is signalled. Running the binary again hands the word to the copy already on
screen — the same path `show`, `hide` and `toggle` take — so the toolbar ends its own run
loop and puts down what it was holding. If nothing was running, this starts a toolbar and
stops it, which is harmless and takes a moment.

Do not offer to restart it. Somebody who asked for it to stop has said what they want.
