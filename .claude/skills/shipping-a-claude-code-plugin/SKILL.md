---
name: shipping-a-claude-code-plugin
description: Use this whenever somebody is building, debugging, publishing or reviewing a Claude Code plugin — a plugin, skill, slash command, hook, agent, monitor or marketplace entry. Reach for it especially when a plugin is written correctly and behaves wrongly: a slash command that does nothing and says nothing, a hook that never fires, a skill Claude refuses to trigger, an install that downloads hundreds of megabytes, a command nobody can find. It carries the failures that look like nothing is wrong, the usability of a plugin as a product, the stream-json runtime underneath, and a lint that checks a plugin against all of it. It complements Anthropic's plugin-dev rather than repeating it, so use both — plugin-dev for what a component is, this for what goes wrong once real people install it.
allowed-tools: Read, Glob, Grep, Bash
---

# Shipping a Claude Code plugin

Verified against **Claude Code 2.1.274**. Where a claim depends on a version, it says so. If
you are reading this on a much later version, re-check the things marked ⚠ rather than
trusting them — a plugin guide that rots silently is worse than one that dates itself.

## Read plugin-dev first

Anthropic ships `plugin-dev` in the official marketplace, and it is good. It has seven skills
and about fourteen thousand words on what each component *is*. Do not ask this skill what
`agents/*.md` frontmatter accepts; ask that one.

```
claude plugin marketplace add anthropics/claude-plugins-official
claude plugin install plugin-dev@claude-plugins-official
```

| Question | Read |
|---|---|
| What goes where? What does the manifest accept? | `plugin-dev:plugin-structure` |
| How do I write a skill? | `plugin-dev:skill-development`, then `skill-creator` |
| Hook events, their input and output shapes | `plugin-dev:hook-development` |
| Subagents | `plugin-dev:agent-development` |
| User-configurable settings | `plugin-dev:plugin-settings` |
| Bundling an MCP server | `plugin-dev:mcp-integration` |

**This skill is the other half**: what happens when the files are all correct and the thing
still does not work, which is where the hours go.

## Pick the right surface first

Most authoring mistakes are choosing the wrong surface, not misconfiguring the right one.
Decide by what you are trying to *cause*:

| You want… | Use | Note |
|---|---|---|
| Claude to know how to do something, when it comes up | `skills/<name>/SKILL.md` | The default. Model-invoked *and* user-invocable. |
| A thing the person types | the same skill | `/plugin:name`. A separate `commands/` file is the legacy layout ⚠ — see below. |
| Something to run deterministically, before the model sees anything | a `!` line inside that skill | Runs first, every time. Not a decision the model makes. |
| To react to what Claude or the person just did | `hooks/hooks.json` | 33 events. Read `references/traps.md` on channels first. |
| A separate context with its own budget | `agents/*.md`, or `context: fork` on the skill | |
| To watch something continuously and report | `monitors/monitors.json` | Experimental ⚠, and deadlined: 30 min, 10 min under `-p`. |
| To expose tools, resources or prompts | `.mcp.json` | |
| A language server | `.lsp.json` | The user must already have the binary. |

A plugin root may hold `.claude-plugin/`, `commands/`, `skills/`, `agents/`, `hooks/`,
`themes/`, `output-styles/`, `monitors/`, `workflows/`, `SKILL.md`, `.mcp.json` or
`.lsp.json`. That list is quoted from Claude Code's own install error, which is the most
reliable place to read it.

**On `commands/`**: Anthropic's own docs call it legacy and point new plugins at `skills/`,
and `plugin-dev:command-development` says so in its own second line. But it is *legacy in
guidance, not legacy in support* — it still loads, and most shipped plugins still use it.
Prefer `skills/`. Choose `commands/` knowingly, not by accident.

## The failures that look like nothing is wrong

Each of these shipped, to real users, in a real plugin. Full symptoms and provenance in
`references/traps.md` — read it before debugging, not after.

- **A `$` anywhere in a `!` line.** The permission check refuses it as `simple_expansion`
  *before it runs*. The command does nothing and says nothing; the reason appears only in the
  transcript. Anything needing the environment belongs in a script the line calls.
- **Every `.md` in `commands/` is a command**, `README.md` included. Yours will appear in
  everyone's `/` menu.
- **A lockfile makes the installer fetch dependencies.** `package-lock.json`, `bun.lock` and
  `npm-shrinkwrap.json` do; `yarn.lock` and `pnpm-lock.yaml` are skipped. One plugin's 14 MB
  clone became a 603 MB install this way.
- **A misspelled hook event never fires and never errors.**
- **A frontmatter key that differs only in punctuation is discarded silently.**
  `allowed_tools` is not `allowed-tools`, and nothing will tell you.
- **`skills` adds to the default path; every other component path replaces it.** Setting
  `commands` in the manifest stops `commands/` loading.
- **Hook stdout reaches the model on only three events**; `systemMessage` never reaches it,
  ever. Getting this backwards means telling Claude what you meant for the person, or the
  reverse.
- **Nothing else runs at install time.** There is no `postinstall`. Whatever is in the
  checkout is the whole of what the user gets.

## Then run the lint

`claude plugin validate --strict` checks the manifests, and you should run it. It catches one
of the faults above. This catches the rest:

```bash
node "$CLAUDE_PLUGIN_ROOT/skills/shipping-a-claude-code-plugin/scripts/lint-plugin.mjs" .
```

It separates **problems** (will bite somebody) from **notes** (worth a look). It reports
nothing on any of the ~30 plugins in Anthropic's official marketplace, so a finding on yours
means something.

## Then prove it does something

`claude plugin eval` is the real answer and it is newer than most guidance ⚠ (needs 2.1.269+).
It runs your plugin against prompts **with and without it loaded** and scores the difference,
which is the only way to know a skill earns its place rather than just existing. Cases live in
`evals/<case>/prompt.md` plus `graders/*.md`; `regex`, `tool_used`, `tool_order` and
`file_exists` graders are deterministic and free, while `llm` and `baseline` cost money.

```bash
claude plugin eval init      # scaffolds a suite
claude plugin eval .         # with/without ablation, report.html, CI exit codes
```

Do not confuse this with `skill-creator`'s `evals/evals.json` — different mechanism, different
format. See `references/verifying.md`, which also covers what tests structurally cannot see.

## Writing the skill itself

`skill-creator` and `plugin-dev:skill-development` both cover this; two things are worth
repeating because they are where skills fail in practice.

**The description is the entire triggering mechanism.** Claude chooses among every available
skill on that text alone. Say what it does *and* when to reach for it, name the situations in
the words somebody would actually use, and lean slightly pushy — skills are under-triggered
far more often than over-triggered. Then check the length: the listing truncates
`description` + `when_to_use` at **1536 characters**.

⚠ Two official sources disagree here. Claude Code's docs give 1536 for the listing; the
platform Agent Skills guidance gives a 1024-character `description` cap, a 64-character name
limit, and forbids `claude` and `anthropic` in names. That last one is demonstrably not
enforced in Claude Code — Anthropic ships `claude-security` and `claude-md-improver`. Treat
the platform numbers as the portable floor if you want the skill to work on claude.ai too, and
Claude Code's as what is actually enforced here.

**The body is paid for on every use.** Once a skill triggers, its body stays in context for the
rest of the session. Put the decision points and the non-obvious knowledge in `SKILL.md`, and
push everything a reader only sometimes needs into `references/`, one level deep — not nested,
because a reader may skim a nested file instead of reading it.

## A plugin is a product

The difference between a plugin people use and one they install once is almost never the
component types. `references/usability.md` is short and it is the part most authors skip:

- A command should **run**, not ask the model to run something.
- Say what actually happened — three outcomes need three sentences, not one word reused.
- Degrade out loud. One failing call should not take down four working ones.
- Never offer a control that cannot act, and never ship a promise you did not implement.
- **Discoverability is the feature.** A capability nobody is told about is a capability nobody
  uses. `SessionStart` + `systemMessage` is the one place a person is certainly looking.
- Name commands so they can be found. `commands/colai.md` becomes `/colai:colai`.
- `/skill-doctor` will tell you your skill is never invoked. Ask it.

## When you need the runtime

`references/substrate.md` covers what a plugin sits on, and you only need it if your plugin
drives Claude rather than decorating it: the `system/init` preamble and what it advertises,
the control channel, `--permission-prompts`, and where the plugin boundary sits against the
Agent SDK. Two things from it that catch people out:

- **A plugin can only ever ship shell hooks.** Programmatic hook callbacks are an SDK-only
  surface and cannot be distributed in a plugin.
- **`background: true` collapses to synchronous** under `-p` and the SDK. A forked skill that
  fires and forgets in the TUI blocks in headless.

## The discipline that matters most

Verify against the running system, and say which version you verified against.

Every trap in this skill was found by running something, not by reading. The one time I
trusted a grep over the documentation I reported eleven hook events when there are
thirty-three. `claude plugin validate`, `claude plugin eval`, `--plugin-dir` and
`claude agents --json` are all cheap. Use them before you believe anything here.
