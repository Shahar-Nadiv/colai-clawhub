# Where this was left

Written on Linux at `0585bb6`, immediately before moving to Windows. The point of this file
is that the session it came from is a 288 MB transcript that is expensive to resume and mostly
about a Linux binary — this is the part worth carrying.

## The state you are arriving at

The plugin works and is installed-and-forget on Linux: `claude` with no flags has
`/colai:show`, `/colai:quit`, the launcher on PATH, and both hooks. 406 JS tests, 126 Rust
tests, `claude plugin validate . --strict` clean, build warnings at 8.

A mark sent to a conversation you are sitting in arrives **in that conversation**, via Claude
Code's `SendMessage` through a small relay agent (`relay.rs`). Roughly 1–3 s warm and about
2¢ a mark, and it arrives framed as a peer message rather than as your own typing. If the
relay cannot reach the chat, the mark waits in `outbox.rs` and a `UserPromptSubmit` hook hands
it over on your next message there. The rail says which of the two happened.

## The first thing to do on Windows, before anything else

**There is no Windows build, and the launcher will say so.** `bin/colai-toolbar` matches
`Linux-x86_64` only; on Windows it prints "no build for …" and stops, which is the honest
failure rather than a crash. `platforms/` holds `linux-x64` (real) and `darwin-arm64` (an empty
aspiration).

So the port comes first, and the UI is *not* the hard part. Four pieces of platform
integration are, all in Rust:

| what | Linux today | Windows needs |
|---|---|---|
| which window is in front | `_NET_ACTIVE_WINDOW`, `_NET_WM_PID` (`colai_attach.rs`) | `GetForegroundWindow` / `EnumWindows` |
| click-through shaped overlay | `input_shape_combine_region` (`colai.rs`) | `WS_EX_LAYERED` + `SetWindowRgn` |
| screen capture | GDK pixbuf (`colai_capture.rs`) | DXGI Desktop Duplication |
| window list | libX11 (`colai_inspect.rs`) | same API as the front-window one |

Two things make this easier than it looks. `tauri-plugin-global-shortcut` is **already
cross-platform**, so the hotkey needs nothing. And `apply_shape` already refuses on any
platform without an implementation — there is a seam, not a tangle.

**Keep the webview.** It is what makes three operating systems possible, and Tauri uses the
OS's own engine — WebView2 on Windows, WKWebView on macOS. The 291 MB footprint measured on
Linux is a WebKitGTK problem specifically: it is unshared and drags in Mesa and the NVIDIA
driver (50 MB of `libLLVM` alone, in two processes). On Windows the same code should be much
leaner because the engine is already resident for the OS. **Measure it there before assuming
either way** — that number is the first interesting thing Windows can tell us.

## Two known bugs, neither fixed

**The 5.24-second send tax.** `where_it_is_had` (`session.rs`) finds a conversation's working
directory by reading every transcript on disk — 622 MB here — to extract one field.
`colai_send.rs` calls it **twice**, so it is a ~10-second tax on every mark. There is an
ignored test recording the measurement:

```
cargo test -- --ignored what_one_send_spends_finding_a_directory --nocapture
```

The fix is cheap and was designed but not written: the cwd is on about line 32 of *one* file,
and the project directory name under `~/.claude/projects/` encodes it anyway. Glob for
`*/<session-id>.jsonl`, read that one file until the `cwd` field appears, stop. Milliseconds
instead of seconds. `colai_sessions` pays the same scan, which is why the receiver picker is
also slow.

**`what_was_said` accumulates a whole transcript before truncating.** It streams line by line
rather than slurping, but pushes every extracted turn into a `Vec` and only trims to 40 at the
end — so opening a long conversation in the Work panel peaks at the whole conversation's text.
Transcripts here reach 288 MB.

## The memory plan, approved but not started

Measured on Linux: **291 MB proportional, and hiding the toolbar costs nothing** — both WebKit
processes stay alive for a window nobody is looking at. That is the whole opportunity.

The approved plan is to destroy the webview when the toolbar is put away and rebuild it on the
next summon, with a grace period of about a minute so rapid show/hide stays instant.
`ensure_overlay` in `colai.rs` already creates it on demand and is idempotent, so the
machinery exists; what changes is that `colai_release` starts making it *not* there. Expected
idle cost ~70 MB.

Three things must be checked rather than assumed, in this order:

1. **A reply arriving while the webview is gone.** `session.rs` emits to `OVERLAY_LABEL`; an
   emit to a window that does not exist must not error and must not lose the answer. This is
   the one that could silently drop somebody's reply.
2. **The hotkey after a release** — it is the only way back.
3. The Work panel's history, which persists through `rememberWork()`.

Environment levers, measured, worth ~34 MB and not free:
`WEBKIT_DISABLE_COMPOSITING_MODE=1` + `WEBKIT_DISABLE_DMABUF_RENDERER=1` gave −26 MB with the
window still drawing at depth 32. `JSC_useJIT=0` gave another 9 MB and should probably be
declined — the page tracks pointer gestures, which is exactly where interpreted JavaScript
would show. These are Linux-only knobs and may be irrelevant on Windows.

## What else to carry

- **`~/.claude/skills/refactor/`** and **`~/.claude/skills/shipping-a-claude-code-plugin/`** —
  copy both folders to `%USERPROFILE%\.claude\skills\`. The second one is the plugin-authoring
  skill, and its `references/traps.md` is exactly the material the Windows port will need.
- **`~/Desktop/claude-code-plugin-craft`** is committed with **no remote** and will be
  stranded. Create an empty GitHub repo and push it, or copy the directory across.
- The plugin is installed per-machine; reinstall on Windows with
  `claude plugin marketplace add Shahar-Nadiv/colai-clawhub@colai-claud-plugin`.

## Things that were tried and ruled out, so nobody spends the time again

- **Injecting into a running Claude Code session**: the Agent SDK has no call for it
  (`createSession()` with `send`/`stream` was removed in TS SDK 0.3.142); the peer socket is
  private and credentialed and answers nothing to newline-delimited JSON; synthesised
  keystrokes cannot aim at one session among several sharing a window; `TIOCSTI` is off by
  default on current kernels. `wrap.rs` — a pty wrapper — works and is parked, because it
  costs starting Claude Code a different way.
- **Shrinking the plugin's files** to help memory. 3.4 MB off a 291 MB footprint is noise.
- **Forcing hardware compositing** to shed `libLLVM`: it stayed at 50.6 MB and cost 9 MB more.
- **Native GTK/cairo UI**: would be genuinely lean (~45 MB) and Linux-only forever, which the
  three-OS requirement rules out.
