# The failures that look like nothing is wrong

Verified against Claude Code 2.1.274.

Every entry here shipped — in a real plugin, to real users. They are collected because they
share a shape: the files are valid, the validator passes, the plugin installs and loads, and
then it does not work. Nothing errors. That combination is what makes them expensive.

Each entry gives the **symptom first**, because the symptom is what you arrive with. Then the
rule, then how it was found — "measured" or "read", because that distinction is the whole
reason to trust or re-check any of it.

**Contents**

1. [Commands and `!` lines](#1-commands-and--lines)
2. [What travels into an install](#2-what-travels-into-an-install) — including how much
3. [Hooks](#3-hooks)
4. [Frontmatter and manifests](#4-frontmatter-and-manifests)
5. [Processes and environment](#5-processes-and-environment)
6. [Distribution](#6-distribution)
7. [Things you cannot have](#7-things-you-cannot-have)

---

## 1. Commands and `!` lines

### My command does nothing, and says nothing

**Rule.** A `!` line may not contain `$`. It is permission-checked *before* it runs and
refused as `simple_expansion`.

The reason never reaches the person. It appears in the session transcript as:

```
Shell command permission check failed for pattern
"!`my-tool show --pidfile "$HOME/..." --in "$CLAUDE_CODE_SESSION_ID"`":
Contains simple_expansion
```

**What to do instead.** Make the `!` line bare and move every environment-dependent decision
into the script it calls. A launcher shell script can read `$HOME` freely; the `!` line
cannot.

```
!`my-tool show`          ← the script derives its own paths
```

**Measured.** Found by running it: the command did nothing and the transcript carried the
reason.

### The model decided not to run my command

**Rule.** A command body is a *prompt*. Writing "run exactly this" over a fenced block makes
the model decide whether to obey — a decision that was never meant to be one. A `!` line
executes before the model sees anything.

**Measured.** A command written the first way sometimes ran and sometimes explained itself
instead.

### `${CLAUDE_SESSION_ID}` is empty

**Rule.** That substitution does not happen in a `!` line. What exists is
`CLAUDE_CODE_SESSION_ID`, in the environment of anything the command spawns — so read it in
the script, not in the line.

**Measured.** By passing it, getting an empty string, and then reading the value back out of
`/proc` to confirm which name actually propagates.

### My README is in everybody's slash menu

**Rule.** Every `.md` in `commands/` becomes a listable, typeable command. `commands/README.md`
registers as `/yourplugin:README` for every person who installs.

**Read, then confirmed** — `claude plugin validate --strict` reports it. It is the one fault
in this document the official validator catches. Keep developer notes in `docs/`.

### `/myplugin` says "Did you mean /color?"

**Rule.** Commands are namespaced `plugin:file`. `commands/colai.md` inside a plugin named
`colai` is `/colai:colai`, and the bare `/colai` resolves to nothing.

**Measured**, by asking a session what a probe plugin had registered.

---

## 2. What travels into an install

### Installing my plugin downloaded hundreds of megabytes

**Rule.** However a plugin arrives — a clone from a git source, a copy from a local path —
Claude Code ends up with a directory. If that directory contains `package-lock.json`,
`bun.lock` or `npm-shrinkwrap.json`, it runs a frozen dependency install over the manifest
beside it. `yarn.lock` and `pnpm-lock.yaml` are skipped. It always
passes `--ignore-scripts`, so this is a download rather than an execution risk.

In the case this comes from, the manifest beside the lockfile belonged to an unrelated npm
wrapper, and a 14 MB clone became a **603 MB install** for people who wanted a shell script.

**Measured**, and narrowed properly: four probe plugins established that the lockfile alone is
the trigger and a `package.json` with dependencies is not.

### Installing from a local path filled my disk

**Rule.** `claude plugin install` from a **local path** copies the directory wholesale —
including everything git ignores. A **git source** clones, so it carries only what is
committed. These are wildly different amounts of data for the same plugin.

Measured on one repository, same commit, same plugin:

| how it was installed | what it carried |
|---|---|
| local path | 619 MB, plus 5.3 GB of `target/` |
| git clone | **7.1 MB** |

`node_modules` was 602 MB of that 619, and the Rust build directory was the rest. The install
failed with `ENOSPC` partway through copying `target/`, having taken the disk from 7 GB free
to 838 MB.

**What to do.** Install from the git source — `owner/repo@branch` — which is also the path
your users will take, so it is the one worth testing. Reach for a local path only in a
repository with nothing large and ignored in it, and remember that `--plugin-dir` exists for
development and copies nothing at all.

**Measured**, by running it and watching the disk fill.

### Half of what I ship is never used

**Rule.** A clone carries everything committed, and for most plugins that is mostly source,
tests and README assets — none of which the plugin needs in order to run.

Measured on the same repository:

```
  .claude-plugin     0.01 MB
  commands           0.01 MB
  hooks              0.02 MB
  bin                0.01 MB
  platforms          3.98 MB   (a compressed native binary)
  ------------------------------
  needed             4.03 MB
  installed          7.43 MB   — 46% was source, tests and a README video
```

The surprise in that table was `toolbar/ui/` — 500 KB of HTML, CSS and JavaScript that turned
out to be **compiled into the binary** by the build, so shipping the sources changed nothing
at runtime. Proved by putting the binary alone in an empty directory and watching it draw.
Worth checking before assuming any asset directory earns its place.

**How to ship less** without deleting anything from the repository:

- A **release branch** holding only the runtime files, named in the marketplace source. Costs
  nothing to a user who already types `@branch`, and a script can publish it.
- A **`git-subdir` source** (`url`, `path`, `ref`), which sparse-clones one directory — so the
  plugin's shippable files live in a subdirectory and nothing else is fetched.
- **`metadata.pluginRoot`** in `marketplace.json`, which moves the plugin root without moving
  the repository around it.

What does *not* work: `.gitignore` (a clone honours it, but the files you want excluded are
usually tracked), and `files` in `package.json` (that is npm's mechanism and Claude Code does
not read it).

### Nothing runs at install time

**Rule.** There is no `postinstall` and no install hook of any kind, by design. Whatever is in
the checkout is the entirety of what the user receives. Anything that must be prepared has to
be prepared on first *use*, from inside the install, with no network.

What does run at install: a lockfile dependency install (above), and a `command`-source
marketplace entry, which requires explicit acceptance and is shown first. What never runs:
skill and agent markdown (loaded as instructions), hook commands (only on their event),
monitor commands (only on their condition), and npm or git lifecycle scripts.

**Read** (documented), and consistent with everything measured.

### `bin/` exists but my command is not found

**Rule.** A plugin's `bin/` is added to PATH **for the session** — so a command file can name
the executable plainly. It is not on PATH in the user's own shell, and `bin/` is not permitted
at all in plugins distributed through claude.ai organisation settings.

**Measured** with `command -v` inside a live session.

### The digest went in the filename

**Rule, and a genuinely obscure one.** If you unpack a binary into a cache, put the version
digest in a **directory** name, never the executable's. X11 takes a window's `WM_CLASS` from
the executable's basename, so `my-tool-3e18b97d` makes a GUI announce itself as a brand new
application after every release — losing its icon, its taskbar grouping and any window rule
the user had written.

Write to a temporary name, verify the digest, then `mv` into place: rename within a directory
is atomic where writing 13 MB is not, and two sessions can ask at once.

**Measured** — with `xwininfo`, after shipping it wrong.

---

## 3. Hooks

### My hook never fires

**Rule.** A misspelled event name does not error. It simply never fires, so the hook looks
installed and does nothing. There are **33** events:

`SessionStart` `Setup` `UserPromptSubmit` `UserPromptExpansion` `PreToolUse`
`PermissionRequest` `PermissionDenied` `PostToolUse` `PostToolUseFailure` `PostToolBatch`
`Notification` `MessageDisplay` `SubagentStart` `SubagentStop` `TaskCreated` `TaskCompleted`
`Stop` `StopFailure` `TeammateIdle` `InstructionsLoaded` `ConfigChange` `CwdChanged`
`DirectoryAdded` `FileChanged` `WorktreeCreate` `WorktreeRemove` `PreCompact` `PostCompact`
`PreModelSwitch` `PostModelSwitch` `Elicitation` `ElicitationResult` `SessionEnd`

**Read** (documented), then confirmed — 30 of the 33 appear verbatim in the 2.1.274 binary.

A cautionary note on that number: an earlier pass grepped the binary with an anchored pattern,
found eleven, and reported it as complete. It was a third of the answer. Grep narrows silently.

### I told Claude something and the person saw it, or the reverse

**Rule, and the most consequential in this document.** A hook has two output channels and they
are not interchangeable.

- **stdout** reaches the **model**, on `UserPromptSubmit`, `SessionStart` and
  `PostModelSwitch`. On every other event, stdout goes only to the debug log.
- **`systemMessage`** (a JSON field) reaches the **person**, in the transcript, and Claude
  never sees it.

So a "your plugin is installed, press X" notice must be `systemMessage` — printed on stdout it
would tell Claude about your plugin and show the user nothing. And context you want Claude to
act on must be stdout — as `systemMessage` it is decoration.

**Read**, then confirmed by shipping one of them the wrong way round.

### `hooks.json` needs no registration

**Rule.** `hooks/hooks.json` at the plugin root is auto-loaded by being there. No manifest key,
nothing to declare. Plugin hooks *merge* with user, project and skill-frontmatter hooks rather
than overriding them.

### `SessionStart` fires far more than you think

**Rule.** It fires on startup, resume, `/clear`, compact **and** fork — with
`session_start_type` naming which. So it must be cheap and idempotent. A notice hook should
re-check the world each time rather than assume it is the first run.

### My hook hung somebody's prompt

**Rule.** Give every command hook a `timeout`. `UserPromptSubmit` runs in front of every
message a person sends; a command that hangs, hangs them.

### The staged thing arrived twice and was acted on twice

**Rule.** A hook that delivers something must be idempotent. It can run between a file being
half-written and finished, and if it does not remove what it delivered, the next message
delivers it again — and context the model acts on, delivered twice, is acted on twice.

Build the payload under a name the hook skips (`*.part`), rename it into place when whole, and
delete it after handing it over.

### A hook that says nothing should print nothing

**Rule.** It runs in front of every message. "Nothing to report" multiplied by every turn is
noise, and noise in that position is expensive.

---

## 4. Frontmatter and manifests

### The setting I set is not set

**Rule.** An unrecognised frontmatter key is discarded silently at load. `allowed_tools` is not
`allowed-tools`; `when-to-use` is not `when_to_use`. The failure is invisible in every
direction except behaviour.

Note that the frontmatter is *extensible* — Anthropic's own skills carry keys the Claude Code
docs do not name, and `metadata` exists so authors can add their own. So an unknown key is not
automatically wrong. A key that differs from a real one only in punctuation almost always is.

### The end of my description never reaches Claude

**Rule.** `description` + `when_to_use` are truncated at **1536 characters** in the skill
listing, which is the only thing Claude sees when deciding whether to use the skill.

⚠ **Two official sources disagree.** The platform Agent Skills guidance gives a 1024-character
`description` cap, a 64-character name limit, and forbids `claude` and `anthropic` in names.
That last is demonstrably not enforced in Claude Code — Anthropic ships `claude-security` and
`claude-md-improver`. Treat the platform numbers as the portable floor for claude.ai, and
Claude Code's as what is enforced here.

### Setting a component path stopped that component loading

**Rule.** `skills` in the manifest **adds to** the default `skills/` directory. Every other
component path — `commands`, `agents`, `workflows`, `outputStyles` — **replaces** its default.
Set one and the default location goes dark.

### My components are in `.claude-plugin/` and nothing loads

**Rule.** Only `plugin.json` and `marketplace.json` live in `.claude-plugin/`. Every component
directory lives at the **plugin root**.

Also: the directory is `output-styles/` (hyphenated) while the manifest key is `outputStyles`
(camelCase). They do not match on purpose and a camelCase directory is not found.

### Install resolves a version that does not exist

**Rule.** `plugin.json`'s `version` and the marketplace entry's `version` must agree.

---

## 5. Processes and environment

### My long-running program dies when the session ends

**Rule.** A program started from a command is an ordinary child and dies with its parent,
which from outside is indistinguishable from one that never started. The obvious fix is
`setsid`, and the sandbox refuses it as something it cannot statically analyse — so **the
program must put itself into its own process group at startup**. Do not put `setsid`, `&` or
`nohup` in the command; they are refused, redundant, or lose the exit status.

The refusal is the better design: a program that only survives when launched a particular way
is fragile in a way nobody can see.

**Measured** — by hitting the sandbox's refusal directly.

### It dies with a symbol lookup error before doing anything

**Rule.** Anything a command spawns inherits the environment of whatever launched Claude Code.
Launched from a snap-packaged editor, that includes the snap's `LD_LIBRARY_PATH`, `GTK_PATH`
and friends, and a native binary dies with `undefined symbol: __libc_pthread_init` before it
draws anything.

Two lessons beyond the bug. **Strip entries, never whole variables** — deleting
`LD_LIBRARY_PATH` takes the user's own paths with it. And **match every mount point**: a guard
that only looked for `/snap/` missed `/var/lib/snapd/snap`, where snapd mounts on Fedora and
openSUSE, so the fix silently did nothing there.

This was fixed three times in three wrong places before being fixed in the program itself,
which is the general lesson: fix it where the knowledge lives, not at each call site.

**Measured** — reproduced from a snap-packaged editor.

### A child of my plugin registered as a nested session

**Rule.** A `claude` spawned from inside a Claude Code session inherits ten session-identifying
variables and comes up as a *child* of that session rather than a conversation of its own — so
it never registers, never appears in `claude agents --json`, and cannot be found. Unset those
ten (`CLAUDECODE`, `CLAUDE_CODE_SESSION_ID`, `CLAUDE_CODE_CHILD_SESSION`,
`CLAUDE_CODE_ENTRYPOINT`, the two `MESSAGING` ones, `CLAUDE_CODE_BRIDGE_SESSION_ID`,
`CLAUDE_CODE_SESSION_ATTENDED`, `CLAUDE_CODE_SSE_PORT`, `CLAUDE_PID`) and nothing else —
`CLAUDE_CONFIG_DIR` is the user's own configuration.

And matching that child by its immediate pid fails: what you exec forks again, so the
registered session is a grandchild. Walk the tree.

**Measured**, after an hour of a socket that never appeared.

---

## 6. Distribution

### `Marketplace file not found`

**Rule.** `claude plugin marketplace add owner/repo` clones the **default branch**. If your
plugin lives on another branch, the source must name it:

```
claude plugin marketplace add owner/repo@my-branch
```

**Measured**, by installing from GitHub end to end — which is the only way to find this class
of fault. Test the real install path, from a real clone, before publishing.

### Who is checking my plugin

**Rule, worth knowing before you plan for it.** The official marketplace is curated by
Anthropic at its discretion and **has no application process**. The community marketplace
accepts submissions after review, where review means `claude plugin validate` plus automated
safety screening; approved plugins are pinned to a commit and the pin auto-bumps as you push,
so only initial acceptance is a gate. The catalogue syncs nightly.

The docs are blunt that none of this makes a plugin trustworthy: plugins "can execute
arbitrary code on your machine with your user privileges."

---

## 7. Things you cannot have

Knowing these early saves designing around them.

- **A plugin cannot ship a keybinding**, and no Claude Code keybinding can invoke a command —
  actions are a closed enum of built-ins. So even by hand, a user cannot bind a key to
  `/yourplugin:thing`. If you need a key, grab one outside Claude Code and *announce* it from
  a `SessionStart` hook.
- **A plugin cannot ship a programmatic hook.** Callback functions are an Agent SDK surface,
  in the host application's process. A plugin's hooks are always shell commands.
- **A plugin cannot inject into a running session** by any documented means. See
  `substrate.md`.
