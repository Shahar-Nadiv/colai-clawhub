## What and why

<!-- What does this change, and why is it needed? Link the issue it fixes, if any. -->

## How it was tested

<!-- What you ran, and on which OS. Screenshots or a short recording help for UI changes. -->

## Checklist

- [ ] `npm test` passes
- [ ] `cargo test` passes (in `toolbar/src-tauri`) and `npm run check:macos` is clean, if Rust changed
- [ ] Tests added or updated for the behaviour this changes
- [ ] `CHANGELOG.md` updated under **Unreleased**, if users would notice
- [ ] Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/)
- [ ] No binaries committed (prebuilt toolbars are staged only when a release is cut)
