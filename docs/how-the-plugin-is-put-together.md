# How the Claude Code plugin is put together

Four facts about Claude Code shape every file in this plugin. Each was checked against the
shipped binary rather than assumed, and the third one was assumed wrong first.

## Commands are namespaced by the plugin

A file at `commands/show.md` is reached as `/colai:show`, not `/show` and not `/colai`.
Typing `/colai` answers *"Unknown command: /colai. Did you mean /color?"*, which is what
sent this looking. So the file is not called `colai.md` — that would be `/colai:colai`.

Every `.md` in `commands/` becomes a command, including one called `README.md`. That is why
this note lives in `docs/` instead: as a sibling of `show.md` it registered as a real,
listable `/colai:README` that would have shipped to everybody.
`claude plugin validate . --strict` is what catches it.

## `bin/` is on PATH, and nothing runs at install time

A plugin's `bin/` directory is added to PATH for the session, so `show.md` can name
`colai-toolbar` plainly. There is no `postinstall` and no install hook of any kind — by
design — so whatever the clone contains is the whole of what the user gets.

That is the reason `bin/colai-toolbar` is a shell script and not the binary. The toolbar has
to be found or unpacked on first use, from inside the install, with no network. See the
comments in that file; the short version is that the 5 MB archive is tracked in git, the
13 MB binary it unpacks to is not, and the digest beside it is what says they are the same
thing.

## The toolbar has to detach itself

`colai-toolbar show` runs an event loop and never returns. As an ordinary child of the
session it dies when the session ends, which from the outside is indistinguishable from a
toolbar that never opened.

The obvious fix is `setsid`, and Claude Code's sandbox refuses it — *"cannot be statically
analyzed"*. That refusal turned out to be the better design: an overlay that only survives
when it is started a particular way is fragile, and the fix belongs in the program. The
binary puts itself into a process group of its own at startup, so it survives however it was
launched. `COLAI_FOREGROUND=1` opts out, for a debugger.

Do not add `setsid`, `&`, or `nohup` to `show.md`. It does this itself.

## The session's environment comes with it

Bash tool calls inherit the environment of whatever launched Claude Code. Launched from a
snap-packaged editor, that includes the snap's `LD_LIBRARY_PATH`, and the toolbar dies
before it draws with a `symbol lookup error` about `libpthread`.

This was fixed three times in three places before it was fixed in the right one. The binary
now drops snap entries from those variables and re-executes itself clean, whoever starts it —
entries, not whole variables, so the user's own paths survive.

## The marketplace

`.claude-plugin/marketplace.json` is what makes `claude plugin install` work; without it the
only way in is `--plugin-dir`, which nobody discovers. The repository is its own marketplace:
one entry, `"source": "./"`, pointing at the plugin beside it.

Both manifests are checked by `claude plugin validate <path> --strict`, and both should stay
that way.
