# What a plugin sits on

Verified against Claude Code 2.1.274. Read this only if your plugin **drives** Claude rather
than decorating it — a slash command, a hook and a skill need none of it.

Claims here are tiered, because some of this is documented and some is not:

- **[docs]** — `code.claude.com/docs` or `platform.claude.com/docs`
- **[measured]** — observed directly on this machine
- **[community]** — a filed issue or third-party write-up, **not** confirmed by Anthropic. Do
  not build anything load-bearing on these without checking them yourself.

---

## The protocol

Claude Code's non-interactive mode is what the Agent SDK drives, and you can drive it too:

```
claude --print --input-format stream-json --output-format stream-json --verbose
```

**[docs]** Newline-delimited JSON in both directions, each frame carrying a `type`. Useful
flags: `--include-partial-messages` (assistant deltas), `--include-hook-events` (interleaves
hook lifecycle frames — note `Notification`, `SessionEnd`, `PreCompact` and `PostCompact`
never emit `hook_started`), `--replay-user-messages` (echoes your input back, for
acknowledgement), `--no-session-persistence`.

### The `system/init` preamble

**[docs, tail fields community-corroborated]** The first frame is
`{type: "system", subtype: "init", …}` and it advertises what the session can do:
`session_id`, `cwd`, `tools`, `mcp_servers`, `model`, `permissionMode`, `slash_commands`,
`skills`, `plugins`, `output_style`, `apiKeySource`, `claude_code_version`, and — since SDK
0.3.229 — `terminal_slash_commands` naming the subset bound to a terminal.

Two details that matter if you read it:

- `skills` lists **user-invocable skills only**. A skill with `user-invocable: false` loads
  and is absent from that array.
- `slash_commands` excludes terminal-only commands like `/theme`.

**[measured] `init` does not arrive until a message is sent.** A child started and left alone
emits its hook frames and then blocks on stdin forever; the preamble belongs to a turn, not to
starting up. If all you want is the advertisement, send one character and kill the child when
`init` arrives — the preamble is written before the model is called, so nothing is billed and
no transcript is written:

```
init after 0.92s with 101 commands — killing now
new transcripts written: 0
```

That is how to populate a menu of the user's real slash commands without spending a turn.

### The control channel

**[community — the wire names are not in Anthropic's prose docs]** Before running a tool the
CLI sends a `control_request` with `subtype: "can_use_tool"` and blocks for a
`control_response`. Other reported subtypes: `initialize`, `hook_callback`, `interrupt`,
`set_permission_mode`, `set_model`.

**[docs]** The same capabilities exist as named SDK surfaces, and if you are writing against
the SDK rather than the raw pipe, use these instead:

| Capability | SDK surface | Note |
|---|---|---|
| Approve or deny a tool call | `canUseTool` callback | Only fires for calls not already resolved by hooks, deny/ask/allow rules or the permission mode |
| Change mode mid-session | `setPermissionMode()` | Works on a streaming query |
| Stop a turn | interrupt on a streaming query | Better than killing the child, which discards the `result` frame and with it the cost accounting |
| Put files back | `rewindFiles()` | Tracks `Write`/`Edit`/`NotebookEdit` only — **not** files a Bash command changed — and restores files without rewinding conversation history |

**[docs]** `--permission-prompts {host|none}` (2.1.259+) decides what happens to a prompt with
nobody to answer it: `host` routes it to your handler, `none` denies. **Choose explicitly.**
The default needs a handler you may not have, and the result is every prompt denied — the same
outcome as `none`, arrived at by accident.

**[measured]** Under `--permission-mode acceptEdits`: a write inside the working directory
succeeds, a write outside it is denied, and Bash is blocked entirely. The working directory
*is* the boundary — which means a conversation whose directory you do not know should get
`default`, not whatever directory you happen to have been launched from.

**[measured]** An unrecognised value for a permission-mode setting should be **refused**, not
passed through. An unrecognised mode silently becoming more permissive is the wrong direction
for a mistake to fall.

---

## Where the plugin boundary is

**[docs]** The Agent SDK is the same runtime, not a reimplementation — so a plugin is the same
artefact on both sides. But the seams are not symmetrical, and these four asymmetries are
where people lose time.

### A plugin can only ship shell hooks

The SDK has two hook mechanisms: the shell-command form in `hooks.json`, and **programmatic**
callbacks passed as `options.hooks`, which return structured decisions
(`permissionDecision: deny|allow|ask|defer`, `updatedInput`, `additionalContext`,
`async: true`).

The programmatic form lives in the host application's process. **A plugin has no way to ship
a callback** — only a command. If your design needs `defer`, or to mutate a tool's input in
process, that is an SDK integration, not a plugin.

### Skills cannot be registered in code

**[docs]** There is no programmatic skill API. Skills are discovered from the filesystem via
`settingSources` (`user`, `project`, `local` — default all three; `[]` for fully
programmatic). Agents, by contrast, *can* be passed in code via `agents`.

The `skills` option takes `"all"`, exact names, or `[]`. Exact names only — `"docs:*"` is
rejected before the session starts. Setting it auto-adds the `Skill` tool to `allowedTools`,
but if you also pass an explicit `tools` allowlist you must add `"Skill"` yourself.

And a quirk worth knowing: **dispatch by name ignores the allowlist.** A skill excluded from
`skills` can still be run as `/its-name`; only the model-invoked path is restricted.

### `plugins` from the SDK is local-only

**[docs]** `plugins: [{ type: "local", path }]` — there is no marketplace-name loading. `~` is
not expanded, and a path that does not exist is **silently skipped** rather than erroring, so
a typo looks like a plugin that does nothing. Verify against the `plugins` array in
`system/init` rather than assuming it loaded.

### `background: true` collapses to synchronous when headless

**[docs]** This is the one that surprises people. A skill with `context: fork` and
`background: true` fires and forgets in the TUI. Under `-p` or the SDK, **Claude Code always
waits** — as it also does when `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` is set, when a prior
invocation of the same fork is still running, or when a scheduled task fires it.

So a skill designed around not blocking will block in every headless context. Also note a
forked skill's edits fall **outside** the session's checkpoint boundary, so `rewindFiles()`
will not undo them.

**[docs]** Some inputs are read regardless of `settingSources`: managed policy,
`~/.claude.json`, auto-memory, claude.ai MCP connectors, sandbox credential masks. If you are
building something locked-down, that list is the floor.

---

## Reaching a session that is already running

Worth stating because it looks easy and is not, and the dead ends are expensive. **[measured]**
in all four cases.

- **The Agent SDK has no call for it.** `resume` and `continue` both start a *new process*
  against the transcript — which means two processes writing one file, the second answering
  where nobody is looking. The one API that resembled it, `createSession()` with
  `send`/`stream`, was removed in TypeScript SDK 0.3.142.
- **The peer socket exists and is private.** `/run/user/<uid>/cc-socks/<pid>.sock` is
  newline-delimited JSON and requires the session's `peerToken` from
  `~/.claude/sessions/<pid>.<hash>.key`. It is not documented and can change in any release.
- **Synthesised keystrokes cannot aim.** Several sessions can share one terminal window — three
  inside one editor, behind one X window — so keys land in whatever pane has focus.
- **`TIOCSTI` is disabled.** `dev.tty.legacy_tiocsti = 0` on every current kernel, and turning
  it on is a system-wide regression.

What does work, in descending order of how much it asks of the user:

1. **Own the terminal.** A wrapper that opens a pty, execs the real `claude` inside it and
   relays gives you a pipe you can type into. Free, instant, and arrives as the person's own
   input — at the cost of them starting Claude through you.
2. **`SendMessage`.** A documented tool for one session to reach another. A plugin cannot call
   a tool, but a `claude` can, so a small long-lived agent can relay. Roughly 1–3 s warm and a
   couple of cents a message, and it arrives framed as a peer message rather than as the
   person's own words.
3. **A `UserPromptSubmit` hook.** Stage the payload, let the hook hand it over on the person's
   next message. Free and completely supported; it waits for a keystroke.

**[measured]** Two facts for identifying sessions, if you go down this road. `claude agents
--json` is a supported listing giving `pid`, `sessionId`, `kind`, `name`, `cwd`, `status`, and
costs about 150 ms — so query it at the moment you need an answer rather than on a timer. And
the private registry at `~/.claude/sessions/<pid>.json` distinguishes a person's own terminal
from an SDK-spawned agent by **`entrypoint`** (`cli` vs `sdk-cli`), *not* by `kind` — both say
`interactive`. Getting that backwards means either colliding with somebody's live chat or
refusing to talk to your own agent.
