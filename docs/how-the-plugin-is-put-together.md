# How the Claude Code plugin is put together

Four facts about Claude Code shape every file in this plugin. Each was checked against the
shipped binary rather than assumed, and the third one was assumed wrong first.

## The toolbar starts from a hook, not a chat command

colai is a desktop overlay, and a plugin can start a process in only two ways — a slash
command or a hook. Summoning a GUI by typing `/colai:show` into a chat was the wrong shape
for it, so there is no `commands/` directory at all: the `SessionStart` hook
(`hooks/say-colai-is-here.sh`) starts the toolbar when a session opens, detached so the hook
returns inside its budget, guarded by the pidfile so it never double-launches, and skippable
with `COLAI_AUTOSTART=0`. The binary still accepts `show`/`hide`/`toggle`/`quit` for anyone
who runs it directly.

(The hazard that shaped this earlier: every `.md` in a `commands/` directory becomes a
listable, typeable `/colai:<name>` command — a note called `README.md` would have shipped as
`/colai:README` — which is one more reason this architecture note lives in `docs/`, and why
colai carries no `commands/` directory to trip over. `claude plugin validate . --strict`
catches a stray one.)

## `bin/` is on PATH, and nothing runs at install time

A plugin's `bin/` directory is added to PATH for the session, so the hook can name
`colai-toolbar` plainly. There is no `postinstall` — the only thing that runs is the
`SessionStart` hook, which starts the toolbar — so whatever the clone contains is the whole
of what the user gets.

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
