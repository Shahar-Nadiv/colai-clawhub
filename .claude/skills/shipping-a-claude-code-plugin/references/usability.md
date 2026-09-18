# A plugin is a product

Verified against Claude Code 2.1.274.

The difference between a plugin people keep using and one they install once is almost never
which component types it uses. It is this. Every item below is something a real plugin got
wrong first, which is why it is worth writing down rather than asserting.

## A command should run, not ask

A command body is a prompt. "Run exactly this" over a fenced block hands the model a decision
that was never meant to be one — and sometimes it explains itself instead of acting.

A `!` line executes before the model sees anything. Use it, and let Claude's turn be about
telling the person what happened.

## Say what actually happened

One plugin reported "sent" for three different outcomes: delivered to a live session, queued
on disk awaiting a keystroke, and answered somewhere the person was not looking. Two of those
are not "sent". Somebody sat watching a window nothing was ever coming to.

Three outcomes need three sentences. If you cannot tell which happened, say *that* — "it may
be waiting" beats a confident wrong word.

## Degrade out loud, and in pieces

A picker fetched five things in one `Promise.all`. One unrelated call failed and the whole
list was replaced with an error, discarding four results that had arrived fine.

Fail the part that failed. And say which part: a plugin that reports "could not connect" when
one optional feature is unavailable teaches people to distrust it.

## Never offer a control that cannot act

A model picker that can only ever say "Claude Code chooses its own model" is furniture. So is
a menu heading with nothing under it, and a keyboard shortcut in a tooltip that is not bound
to anything. Each one is a small lie, and people notice the pattern faster than any single
instance.

If a capability is unavailable, hide the control rather than showing it disabled — unless
disabled-with-a-reason genuinely teaches something.

## Never ship a promise you did not implement

A first-run card said the tips could be reopened "from the tray". There was no tray and no way
back. The text was written when there was, and outlived it.

Search your own copy for promises when you remove a feature.

## Discoverability is the feature

The most instructive failure of all: a plugin's global shortcut worked from the first day and
went unused for weeks, because nothing ever said it existed. The mechanism was never the
problem — being told was.

`SessionStart` with `systemMessage` is the one place a person is certainly looking. Use it to
say what you added and how to reach it. Keep it to a line, make it idempotent, and say nothing
when there is nothing to say.

## Name things so they can be found

`commands/colai.md` in a plugin named `colai` becomes `/colai:colai`, and `/colai` answers
"Did you mean /color?". Name the file for the verb: `show.md` → `/colai:show`.

For skills, the `description` is the name as far as Claude is concerned. It is the only thing
consulted when deciding whether to use the skill, so it has to name the situation in the words
somebody would actually use.

## Ask whether anyone uses it

`/skill-doctor` reports what each skill costs and how often it is invoked, and flags the ones
never invoked at all. A skill that never triggers is not a small problem — it is the whole
problem, and it is usually the description rather than the content.

## Test it by running it, not by reading it

This is a usability point, not only an engineering one, because the bugs that survive reading
are exactly the ones a person hits immediately.

One project had 441 passing tests that all read source and asserted on its contents. Three
crashes shipped in a row anyway — each invisible to every one of those tests and obvious to
anyone who opened the thing for one second. One of them meant a menu had never worked since
the first commit, and a person found it, not a test.

Install your plugin from a real clone. Run the command. Press the key. Read what the person
reads.
