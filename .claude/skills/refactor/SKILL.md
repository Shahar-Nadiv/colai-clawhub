---
name: refactor
description: Use this when somebody wants code improved without changing what it does — they say refactor, clean up, tidy, simplify, "this function is too long", "there's duplication here", "this is hard to follow", or they name a file and ask for it to be made better. Also reach for it when a file is about to be extended and its current shape is in the way, which is the most valuable moment to refactor. It works in small verified steps rather than one large rewrite, and it treats "behaviour is unchanged" as something to prove rather than assert. Do not use it when the ask is to change behaviour, add a feature or fix a bug — those are different jobs, and mixing them with a refactor is what makes a refactor unreviewable.
argument-hint: "[file or directory]"
allowed-tools: Read, Glob, Grep, Edit, Bash
---

# Refactor

Improve the shape of code without changing what it does.

The whole value of a refactor is the guarantee that comes with it. Anyone can rearrange code;
the reason to call something a refactor rather than a rewrite is that behaviour is provably
the same afterwards. So the work is organised around being able to prove that, and everything
else follows from it.

## Before touching anything: can you tell if you broke it?

Find out how behaviour is currently verified, and say so out loud.

```bash
# Look for the test command before looking at the code.
cat package.json 2>/dev/null | grep -A3 '"scripts"'
ls Makefile justfile Cargo.toml pyproject.toml 2>/dev/null
```

Then run it, **before changing anything**, and note what passes. A refactor that starts from
an unknown baseline cannot be distinguished from a refactor that broke something.

**If there are no tests covering the target, that is the first finding, and it changes the
plan.** Say it plainly rather than proceeding as though the guarantee holds. Two honest routes:

- Write characterisation tests first — tests that pin down what the code *currently* does,
  including behaviour that looks wrong. Then refactor against them. This is the right answer
  when the code matters.
- Or refactor only what a compiler or type checker can verify — a rename, an extracted
  function with an identical signature — and say explicitly that the rest was left because it
  could not be checked.

Do not claim behaviour is preserved because the changes looked safe. That is the claim this
skill exists to earn.

## Then read, and find the real problem

Read the target fully before proposing anything. Long files reward being read in one pass,
because the duplication worth removing is usually the kind you only notice twice.

Look for these, in roughly descending order of how much they cost a reader:

**Structure**
- The same logic in more than one place — extract a function, and name it for what it means
  rather than what it does mechanically
- A file doing several unrelated jobs — split by responsibility, not by line count
- Code far from where it is used — move it closer; distance is a maintenance cost
- A reusable pattern written out repeatedly — extract a component

**Simplification**
- Nested conditionals — invert them into guard clauses and let the happy path run flat
- Callback chains — `async`/`await` where the language has it
- Unexplained literals — a named constant, with the reason in its name or a comment
- Dead code and unused imports — delete them; code with no caller is worse than absent code
  because it looks maintained

**Patterns**, applied only when the code is already asking for them
- Composition where inheritance is being used for code reuse rather than for a real is-a
- A strategy where a growing `switch` on a type is really several behaviours in a trench coat
- A builder where a constructor has grown too many positional arguments to read
- Early returns to flatten nesting

**Resist the pattern for its own sake.** A strategy pattern over two cases is more code and
less clarity. The test is whether a reader arrives faster afterwards, not whether the result
is more sophisticated.

## Work in steps that can each be undone

One refactoring at a time. After each:

1. Make the change.
2. Run the tests.
3. If they fail, that step was wrong — undo it rather than fixing forward. The failure is
   information: it means behaviour was not what you thought.

This is slower than one large edit and it is the only way the guarantee survives. It also
means that when something does break, the cause is the last thing you did rather than one of
forty things.

## What to leave alone

- **Public API signatures**, unless changing them was explicitly asked for. A refactor that
  changes a signature is a breaking change wearing a friendly word.
- **Behaviour that looks like a bug.** If you find one, say so — do not fix it inside a
  refactor. A commit that both moves code and changes what it does is a commit nobody can
  review, and the bug deserves its own decision.
- **Formatting-only churn in files you are not otherwise touching.** It buries the real diff.

## Committing

Propose the commits; do not make them unasked. One logical change per commit, each message
saying what moved and why — a reviewer reading the log should be able to follow the reasoning
without reading the diff.

If the person wants it committed as you go, they will say so. Offer it once and then follow
their lead: an unrequested commit is harder to undo than an unrequested edit.

## Report what happened

Say, concretely:

- the test command, and that it passed before and after
- each refactoring applied, and what it bought
- anything found and **not** changed, and why — the bug left alone, the untested region, the
  pattern deliberately not applied
- anything you could not verify

That last line matters most. "Tests pass" when there were no relevant tests is the one way a
refactor does real damage.
