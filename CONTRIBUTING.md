# Contributing

Thanks for wanting to make colai better. Bug reports, ideas and pull requests are all welcome —
please read [the Code of Conduct](CODE_OF_CONDUCT.md) first, and report security issues
privately as described in [SECURITY.md](SECURITY.md), never in a public issue.

## Setup

You need **Node 24+** (it ships npm 11 — npm 10 crashes resolving vitest's optional peers),
**Rust stable**, and on Linux the WebKitGTK/GTK development packages Tauri builds against
(see the Linux step in [`.github/workflows/ci.yml`](.github/workflows/ci.yml)).

```bash
npm install
```

## Build and test

```bash
npm test                          # the JS suites (vitest) — a few seconds
cd toolbar/src-tauri && cargo test # the Rust tests
npm run check:macos               # type-check the macOS code, from any OS
npm run build:toolbar             # build a toolbar for this machine into bin/
```

CI runs all of this on Linux, Windows and macOS for every push and pull request; a change
should be green on all three before it is merged.

## Code conventions

- **The UI is classic JavaScript, on purpose.** There is no bundler, no framework and no
  build step for `toolbar/ui/`: the files are classic scripts that share one global scope and
  run in the webview exactly as written. That keeps what ships readable in the repository, and
  the CSP (`script-src 'self'`) depends on it. Top-level names must therefore be unique across
  files (a test enforces it).
- **Agent text never becomes markup.** Anything a model, a transcript or another session
  wrote reaches the DOM through `textContent` / `toolbar-markdown.js`, never `innerHTML`.
- **Platform code is gated, not forked.** Add a `#[cfg(target_os = "…")]` arm; never change
  another platform's behaviour as a side effect. Run `npm run check:macos` after Rust changes.
- **Comments explain why.** The codebase is written to be read; match its voice.

## Commits and pull requests

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/):
  `feat:`, `fix:`, `perf:`, `refactor:`, `docs:`, `test:`, `ci:`, `chore:`, with an optional
  scope — e.g. `fix(hook): see the running toolbar on Windows`. The body says why.
- Keep pull requests focused, add or update tests for behaviour you change, and add a line to
  the `Unreleased` section of [CHANGELOG.md](CHANGELOG.md) when users would notice.
- **Do not commit binaries.** The prebuilt toolbars under `platforms/*/bin/` are produced only
  when a release is cut (below), never in an ordinary change.

## Releasing (maintainers)

1. Bump the version in `package.json`, `toolbar/src-tauri/Cargo.toml`, `tauri.conf.json` and
   both `.claude-plugin/*.json` (a test checks they agree), and date the CHANGELOG entry.
2. Build and stage each platform's binary: Linux with `npm run build:release` (Docker, so the
   binary carries no build-machine paths), Windows with `npm run build:release:windows`
   (builds from a neutral directory and refuses to stage a binary that names its builder),
   macOS by running the **Build macOS toolbar** workflow from the Actions tab (it builds,
   ad-hoc signs and commits the Apple Silicon binary on a GitHub macOS runner) — or on a Mac
   with `npm run build:toolbar` then `npm run stage:macos`.
3. Commit as `chore(release): vX.Y.Z`, tag `vX.Y.Z`, push, and publish a GitHub Release from
   the tag with the CHANGELOG entry as its notes.
