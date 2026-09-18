# Proving a plugin does something

Verified against Claude Code 2.1.274.

Three layers, cheapest first. They check different things and none replaces another.

## 1. `claude plugin validate`

```bash
claude plugin validate . --strict
```

Manifest syntax, required and optional fields, component paths, path traversal, unparseable
agent frontmatter, marketplace rename cycles. `--strict` promotes unrecognised-field warnings
to errors, which is how a misspelled manifest key gets caught.

Run it in your test suite, not just by hand. It is also what the community marketplace's
review pipeline runs, so passing it locally is passing the first gate.

**What it does not check**: anything in `traps.md`. Every fault there is valid on disk.

## 2. The lint beside this skill

```bash
node scripts/lint-plugin.mjs .
node scripts/lint-plugin.mjs --all ~/.claude/plugins/marketplaces/<a-marketplace>/plugins
```

The traps as executable checks. It reports **problems** (will bite somebody) separately from
**notes** (worth a look), and exits non-zero on problems so it can gate a release.

It reports nothing on any of the ~30 plugins in Anthropic's official marketplace. That
calibration was not free — a first version flagged 28 things there, all of them false: it
treated unknown frontmatter keys as errors when the format is extensible, and it read the `!`
examples inside `command-development/SKILL.md` as real `!` lines because it did not skip fenced
code blocks. **Run a new lint against known-good plugins before trusting it**, or you will
train yourself to ignore it.

## 3. `claude plugin eval` — the one that answers "is this worth installing"

Needs Claude Code 2.1.269+.

```bash
claude plugin eval init        # scaffold a suite
claude plugin eval .           # run it
```

This is the layer most plugins skip and the only one that answers the real question. It runs
prompts against your plugin **and again without it**, and scores the difference — so a skill
that changes nothing shows up as changing nothing.

A suite lives in `evals/`. Each case is a directory with `prompt.md` (frontmatter: `name`,
`runs` default 3, `model`, `max_turns`, `timeout_seconds`, `allowed_tools`, …) and a
`graders/` folder, one markdown file per grader.

| Grader | Checks | Cost |
|---|---|---|
| `regex` | output matches a pattern | free |
| `tool_used` | a named tool was called | free |
| `tool_order` | tools were called in order | free |
| `file_exists` | a file was produced | free |
| `llm` | a judge model's opinion | paid |
| `baseline` | comparison against the no-plugin arm | paid |

The four deterministic graders are enough for most plugin assertions — "did it read the
reference file", "did it run the lint", "did it produce the manifest" — and they cost nothing,
so there is no excuse for an empty `evals/`.

In CI:

```bash
claude plugin eval . --trust-plugin --json results.json \
  --threshold 0.8 --model <pinned> --judge-model <pinned> \
  --no-publish --max-cost-usd 20
```

Exit codes: `0` all cases met the threshold, `1` a case failed or the directory was untrusted,
`2` a partial run (cost cap or auth), `130` interrupted, `143` terminated. Each run is a fresh
sandboxed `claude -p` with only your plugin loaded — no personal settings, no CLAUDE.md, no
other plugins — and the eval files themselves are hidden from the agent under test.

**Do not confuse this with `skill-creator`'s `evals/evals.json`.** Different mechanism,
different format, different purpose: that one drives a human review loop for skill *writing*;
this one is a scored gate for a *plugin*.

## What none of these can see

Be honest about this in your own reports, because the temptation is to let a green suite imply
more than it proves.

- **Layout and appearance.** A DOM-executing harness has no layout engine and no CSS engine. A
  test can assert a class is applied and cannot know the element is visible, inside its
  container, or the right colour. One project shipped a logo that rendered *invisible* — the
  class was there, the rule that coloured it was not, and the test passed.
- **Anything behind a keystroke**, unless the harness can produce the field being typed into.
  That gap is how a completion menu stayed broken from a project's first commit until a person
  pressed the key.
- **A regression that is also correct behaviour.** A fallback path silently became the
  permanent path when the thing that used to trigger the primary path stopped existing. The
  fallback was correct and tested. Nothing failed. The feature was a hundredth of itself.
- **Whether the description triggers.** Only running real prompts tells you, which is what
  `plugin eval` and `skill-creator`'s description loop are for.

## Two disciplines worth adopting

**Confirm every test red before trusting it green.** Reintroduce the bug, watch the test fail,
put it back. A test that has never failed has never been shown to test anything — and this is
how an early version of a "no globals used as values" check was found to miss the exact bug it
was written for.

**Delete tests with the features they covered.** A test asserting behaviour that was removed on
purpose is not a regression guard; it is a reminder of what used to be there, and it will be
read as a requirement by somebody later.
