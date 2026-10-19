# Changelog

A Claude Code plugin, from 0.2.0 onwards. Versions **0.1.x** were a different product that
shared this toolbar and this history; nothing of it remains on this branch as of 0.4.0, and
the entries below are left exactly as written. A changelog that is edited to match the present
is not a record of anything.

## Unreleased

## 0.5.5 — 2026-10-19

- **Icons in Claude's own style.** Every key on the rail is redrawn the way Claude Desktop draws
  its icons: a 20-point grid, hairline 1-pixel lines, round ends, one colour.
- **One Artifacts key.** Claude Design, Docs, Slides and Dashboards sit under a single key: hover
  or click it for the menu, hover a kind for its shelf beside it. A shorter rail, the same reach.
- **Quieter popups.** The explanatory lines are gone from the shelves and the New form.
- **The toolbar no longer freezes.** The watcher that lets the rail take the mouse could stop, and
  every click then fell through to the desktop. It is now watched itself: restarted within a few
  seconds if it ever goes quiet, with the reason written to `~/.config/ai.colai.toolbar/colai.log`.
  The rail's clickable area also follows the page's own scale.
- **"Working" ends when the work does.** A mark answered in a Claude Desktop chat kept the
  jellyfish and the chat key working for up to four minutes after the agent had finished. The
  agent's own end of turn now ends the run at once.

## 0.5.4 — 2026-10-17

- **Claude Design, one click away.** Claude Design projects are not artifacts, and nothing a plugin
  can call lists them, so the Design key no longer shows a shelf of Design-type artifacts: it opens
  claude.ai/design, where your projects are.
- **All artifacts.** Each shelf (Docs, Slides, Dashboards) switches between its own kind and every
  artifact you have made, so plain pages of no type are on a shelf too.

## 0.5.3 — 2026-10-17

- **Artifact shelves that read like Claude's gallery.** Two across, a preview, the title, the kind
  and date beneath; New first as a dashed card. Claude does not give plugins an artifact's
  thumbnail, so each preview is a drawing of its kind, varied by the artifact so a shelf reads as
  several things.
- **Real titles.** The per-type listing carries the type's description in each row, which had
  turned every title into that description; titles now read correctly.

## 0.5.2 — 2026-10-17

Your artifacts, on the rail.

- **Designs, Docs, Slides, Dashboards.** Four keys where undo and redo were (Ctrl+Z and
  Ctrl+Shift+Z still work). Each opens a scrollable shelf of your own artifacts of that kind,
  newest first; click one to open it. New, first on the shelf, asks the chat you are sending to
  to make one, from what it should be about and anything you have marked.
- **Listed through your own session.** The bridge asks Claude's Artifact tool from inside your
  Claude Desktop (or terminal) session, so it needs no separate sign-in and costs nothing.

## 0.5.1 — 2026-10-16

- **Remove a tool you made, from its tile.** Hover a tool you made in the tools grid and press ×;
  it asks once (Remove / Keep) before it goes. Colai's own tools cannot be removed.

## 0.5.0 — 2026-10-16

Tools you make yourself.

- **The tools grid.** The ⋯ key opens a popup instead of folding keys out of the rail: a
  scrollable grid with Measure, Colour, Record and Screenshot, every tool you have made, and
  first, New tool. The rail is four keys shorter.
- **Make a tool.** Give it a name, how you mark (point, box, circle, draw or the whole screen), a
  prompt saying what Claude should do with what you mark, and an icon: one of the toolbar's own,
  or a small picture you import. Use it like any other tool; its prompt goes with every mark it
  makes. Change or delete it from the grid with the pencil on its tile.

## 0.4.9 — 2026-10-16

The toolbar and Claude Desktop, side by side.

- **The desk stays clickable.** A marking tool used to stay in hand after the mark, and the
  overlay caught every press on every screen until the mark was sent, so Claude Desktop could
  not be clicked or typed in meanwhile. A mark now puts the tool down. Hold Shift as you finish
  a mark to keep the tool for the next one.
- **The conversation goes on in Claude Desktop.** When a mark reaches a chat open in the desktop
  app, the app is brought forward, and the toolbar's card becomes a receipt (where the mark went,
  whether it has been answered) with Open in Claude Desktop, instead of a smaller copy of the
  reply. Artifacts, diffs and the whole thread are where they belong. Later replies in that chat
  no longer raise toasts or an unread count in the toolbar.

## 0.4.8 — 2026-10-16

Colai looks like part of Claude Desktop, and the reply card tells the truth.

- **Claude's look.** The toolbar now uses Claude's own design tokens instead of the coral and
  blue-grey it was born with: warm neutral surfaces, Claude's clay kept for the one primary
  action, a rounded-rectangle rail like the composer, quiet selection, Claude's fonts with their
  system fallbacks. It follows the system's light or dark theme the way the app does.
- **Honest card states.** A mark handed to a chat shows where it is: Handing over, then In
  <chat>, as your message (or sent by Colai, through the relay), or Waiting for you when it will
  go in with your next message there, in amber and never as a failure.
- **Failures belong to their own card.** A warning from `claude` is a notice, not a failure, and
  one chat's trouble no longer fails another chat's card.
- **Sign-in, explained.** A signed-out `claude` is recognised, and the card says so in one
  sentence with `claude auth login` and a Copy button.

## 0.4.7 — 2026-10-16

Marks go into a Claude Desktop chat as your own message, and approvals ask the way Claude Code
does.

- **The bridge.** A hooks module (`hooks/bridge.ts`) now runs inside each Claude Code session,
  terminal or desktop, and puts a mark addressed to it in as your own prompt: at once when the
  chat is idle, right after the current turn when it is busy. It replaces the relay agent for
  any chat it runs in: no extra turn or cost, no second sign-in, no "message from another
  session". The relay and the next-message hook remain as fallbacks, and a mark the bridge
  delivered is never handed over again.
- **Approvals match Claude Code.** A permission request shows exactly what will run (the
  command, address, query or pattern) with Claude's description beneath, and answers in
  Claude Code's words: Yes; Yes, and don't ask again for <rule> (only when Claude Code offered
  one, and only that rule); No, and tell Claude what to do differently, with a box for what to do
  instead. The modes use Claude Code's names: Ask before edits, Accept edits, Plan mode, Auto.
- **The README no longer promises pictures are never written to disk.** A mark handed to a chat
  you have open is written to `.colai/marks/` in that project, which is how the chat reads it.

## 0.4.6 — 2026-10-15

- **A mark sent to an open chat arrives once.** In the desktop app the relay's message starts a
  turn at once, and that turn's prompt hook found the copy staged for "your next message" still
  there and delivered the mark a second time. The staged copy is now hidden from the hook while
  the relay works, deleted when it delivers, and put back if it cannot.
- **Stop says where to stop a turn colai cannot.** A mark handed to your own chat runs in that
  chat's process, which colai cannot interrupt; Stop stops watching it and now points at the
  right place: the stop button in a Claude Desktop chat, Esc in a terminal.

## 0.4.5 — 2026-10-15

Works from the Claude desktop app.

- **A mark sent to a desktop chat arrives in that chat.** colai only counted a terminal
  (`entrypoint: cli`) as somebody sitting in a conversation, so a desktop chat looked empty and
  colai resumed it in a process of its own: the reply went to the Work panel, nothing appeared in
  the chat, and two processes wrote one transcript. Every session but colai's own children
  (`sdk-*`) is now handed over, as a terminal one always was.
- **The toolbar no longer carries the identity of the session that started it.** Launched from a
  desktop chat's hook, it inherited that chat's variables, including the one telling a session
  its host refreshes its sign-in, and passed them to every `claude` it ran, which then failed with
  "OAuth session expired". Only the session-identity variables are dropped; your own settings stay.
- **The `claude` command must be signed in** (`claude auth login`) even if you only use the
  desktop app, which keeps its own sign-in. The README says so now.
- **One-press close** reaches Linux, whose 0.4.4 build had failed.

## 0.4.4 — 2026-10-05

- **The close key closes on one press.** It took two, the first arming it, which read as a button
  that did not work.

## 0.4.3 — 2026-10-05

- **No more dead rail after showing it again.** Showing (or focusing) the toolbar made the window
  library put "let clicks through" back on, behind the click-through watcher's cached answer, so
  the rail ignored clicks until the pointer left it and came back. The watcher now reads the
  window's real style every tick and corrects it within one frame.
- **No terminal flashing at startup.** Two `claude` probes the toolbar runs when it starts (its
  version, and which slash commands exist) opened a console window on Windows; they are now
  windowless like every other `claude` it starts.

## 0.4.2 — 2026-10-05

- **The rail can be pressed after Ctrl+Alt+Space hides and shows it again.** Hiding forgot where
  the rail was drawn and nothing told the toolbar again, so on Windows every click fell through a
  rail that was plainly on screen.

## 0.4.1 — 2026-10-05

Fixes from the first clean-machine install.

- **The toolbar opens on the session that started it.** The `SessionStart` hook now reads the
  session id Claude Code hands it on stdin and passes it as `--in`; before, the rail opened on
  an empty receiver. A brand-new session has no transcript until its first message, so the rail
  keeps looking until that session appears and then points at it.
- **A close key.** The toolbar outlives the session that started it, on purpose. An × at the end
  of the rail now closes it — two presses, the first arms it — instead of only
  `colai-toolbar quit`.
- **"Session"** labels an unchosen receiver, instead of "Choose who receives" or "Conversation".
- **Windows: the session list is no longer empty when started outside Git Bash.** The toolbar
  looked for conversations under `HOME`, which Windows does not set; it now falls back to
  `USERPROFILE`.
- **A startup message you notice.** It is now a headline with the jellyfish in front and what to
  do on the line beneath, rather than one sentence easy to read past.
- **macOS ships prebuilt.** The Apple Silicon toolbar is now built and ad-hoc signed on a
  GitHub macOS runner and staged under `platforms/darwin-arm64/`, so a Mac installs it the
  same way as Windows and Linux. Still a preview: not yet walked through by hand on a Mac.
- **The startup message says why, when it can't start.** The `SessionStart` hook asks the
  launcher (`colai-toolbar check`) first, and names the missing build, Wayland or missing
  library instead of announcing a toolbar that never appears.

## 0.4.0 — 2026-10-04

Windows, a preview of macOS, and a toolbar that starts itself.

- **Windows (x86-64).** A prebuilt binary ships beside the Linux one; the launcher picks it
  under Git for Windows, and a `colai-toolbar.cmd` shim lets it resolve from cmd and
  PowerShell too. Line endings are pinned so the POSIX launcher survives a Windows clone.
- **macOS (Apple Silicon), preview.** The overlay, click-through, capture, front-window
  tracking and file pickers are implemented and pass CI on a real Mac; there is no prebuilt
  binary yet — build it with `npm run build:toolbar` and `npm run stage:macos`. See
  `docs/macos.md`.
- **No commands to type.** The `SessionStart` hook starts the toolbar when a session opens
  (`COLAI_AUTOSTART=0` to opt out). `/colai:show` and `/colai:quit` are gone; `colai-toolbar
  quit` stops it.
- **Work panel, redesigned.** Each conversation reads as a You/Claude thread with your own
  words rather than the message colai composed; every prompt has **Rewind to here**, which
  previews and then puts back the file edits made from that point on, plus **Copy /rewind**
  for the conversation itself.
- **Replies that arrive later are noticed.** A chat you handed a mark to is followed after
  its first answer; each later reply raises a toast and a red unread count on the Work key.
- **Scheduled tasks**, from a clock key beside Work — once, hourly, daily or weekly, carrying
  a picture of whatever was pointed at.
- **Settings** can be dragged and closed, and show Claude Code's own plugins, skills and MCP
  servers next to colai's.
- **Faster and lighter.** Idle CPU fell from about 27% of a core to about 1%; the
  conversation list is cached instead of re-reading every transcript every five seconds
  (100 ms → under 1 ms), and sending no longer scans them at all.
- **Fixes.** `@` file-mentions find files again; a mirrored turn no longer settles as done
  while a long tool is still running; popups flip to the side with room instead of running
  off the screen.
- **Security.** Agent-authored HTML is rendered with the network disabled; scheduled tasks
  never run with auto-accepted edits in a home, root or credentials directory; the
  permission list matches the commands that exist.
- The Context map is hidden in this release and returns later.

## 0.3.0 — 2026-09-16 (Claude Code)

The plugin can be installed rather than pointed at.

- **A marketplace.** `claude plugin marketplace add Shahar-Nadiv/colai-clawhub` and then
  `claude plugin install colai@colai`. Until now the only way in was `--plugin-dir`, which
  nobody discovers.
- **`bin/colai-toolbar` is a launcher, not the binary.** Claude Code puts a plugin's `bin/`
  on PATH and runs nothing at install time — there is no `postinstall` — so whatever the
  clone contains is the whole of what you get. The 4 MB archive travels in the repository
  and the 11 MB binary it unpacks to does not; the launcher unpacks it into `~/.cache/colai`
  on first use and checks it against the digest beside it every time before running it.
- **595 MB of `node_modules` no longer lands in every install.** Claude Code installs a
  plugin by copying its checkout, and a `package-lock.json` in it makes the installer run
  npm over the manifest beside it — which here is the OpenClaw npm wrapper, so an install
  fetched 328 packages including OpenClaw itself into the plugin directory of somebody who
  wanted a toolbar. The lockfile is gone from this branch: a clone is 14 MB, and an install
  is now what the clone is.
- **A machine with no build is told which machine it is**, rather than being handed an
  x86-64 binary and shown `Exec format error`.
- **`commands/README.md` was a command.** Every `.md` in `commands/` becomes one, so a note
  meant for developers was a listable `/colai:README` in everyone's session. It has moved to
  `docs/`, and `claude plugin validate --strict` now runs in the suite.
- The README described installing an OpenClaw plugin and talking to a Gateway. It now
  describes this one, including the part the Gateway version had no equivalent for: every
  mark costs tokens on your own Claude account.

## 0.2.0 — 2026-09-15 (Claude Code)

The toolbar talks to Claude Code instead of an OpenClaw Gateway.

- The Gateway client, its device identity and its credential bootstrap are gone — 4,903
  lines — replaced by a `claude` process the toolbar owns and speaks stream-json to. It uses
  the login you already have; there is no API key and no second account.
- `/colai:show` puts the toolbar on screen, and it survives the session ending: the binary
  puts itself into a process group of its own, because `setsid` is refused by Claude Code's
  sandbox and an overlay that only survives one way of being started is fragile.
- A snap-packaged terminal used to hand the toolbar its own library paths and kill it before
  it drew. It now drops those entries and re-executes itself clean, whoever starts it.

## 0.1.3 — unreleased

The binary moves out of the plugin and into a package per machine.

- **`@colai/toolbar` no longer carries a binary.** It names one platform package per kind
  of machine as an optional dependency, each declaring the `os` and `cpu` it is for, and
  npm installs only the one that matches. This is how esbuild and swc ship, and OpenClaw
  supports it directly: `openclaw.install.requiredPlatformPackages` makes the host verify
  the matching build actually arrived, retry once with a cold cache, and roll the install
  back if it did not.
- **`os: ["linux"]` is gone from the plugin**, and that is the point of the change: npm
  reads it before it considers a single optional dependency, so it refused a Mac the
  package before the Mac could ever be offered a Mac binary. The refusal now lives in the
  packages that hold an executable.
- **A machine with no build is told which machine it is.** "Cannot find module" sent people
  looking for a failed download; it now says there is no build for `darwin arm64` and names
  what there is.
- The digest compiled into the runtime is one per build. It stays in the wrapper rather
  than moving beside each binary, which makes the gate stronger: the wrapper is a different
  package from the binary it vouches for.

There is still one build, Linux x86-64. Nothing about what the toolbar does has changed.

## 0.1.2 — 2026-09-15

A second audit, and the half of the first fix that was missing.

- **An attached file can no longer forge the block either.** 0.1.1 stopped a window title
  closing `<observed>…</observed>`, which left the fields *outside* that block — an
  attachment's name and path, the repository a git mark is about — free to open one of
  their own. That is the better attack, not a safer one: an unclosed block swallows the
  instruction and your own sentence into a region announcing that nothing inside it is an
  instruction, and the attachment list is composed first, so the forgery goes first. A
  Linux filename may contain `<` and `>`, so a downloaded archive was enough to plant one.
  Angle brackets now go at the single point every composed field already passes through.

## 0.1.1 — 2026-09-15

An audit before the first public release, and what it found.

- **A window title can no longer close the block it is quoted inside.** Facts read off
  the desktop are wrapped in `<observed>…</observed>` so an agent can tell them from an
  instruction. A browser's window title is the page's own `<title>`, chosen by whoever
  wrote the page — and a title containing `</observed>` ended the block early, so
  everything after it arrived in the same voice as the real instruction. Angle brackets
  are now removed from everything inside the block.
- **TLS updated.** rustls 0.23.43 → 0.23.45, for RUSTSEC-2026-0285, in which TLS 1.3
  handshake messages could be accepted across encryption level boundaries. It affects a
  Gateway reached over a network far more than one on loopback.
- **The toolbar is unpacked more carefully.** The temporary file refuses to follow a
  symlink left in its place, and a corrupt or hostile archive is refused rather than
  decompressed without limit.
- **The snap library guard was wrong twice.** It missed `/var/lib/snapd/snap`, which is
  where snapd mounts on Fedora and openSUSE — so on those the fix did nothing. And when
  it did fire it discarded the whole variable, taking any of your own entries with it.
  Now only the snap entries are removed.
- **One scope fewer.** The toolbar asked the Gateway for `operator.pairing` and never
  used it.
- The README and this file both said the toolbar's only network connection is your own
  Gateway. The component library's preview pictures come from `cdn.21st.dev`. Corrected
  in both.

## 0.1.0 — 2026-09-14

First public release.

Point at anything on your screen and hand it to an OpenClaw agent: box it, point at it,
measure it, sample a colour off it, record a few seconds of it, then say what you want and
send it with a picture of exactly what you meant.

### What it sends, and what it does not

- URL query strings and fragments are removed before a mark leaves the machine. That is
  where session tokens, signed-link signatures and email addresses live.
- `/home/<someone>` is rewritten as `~` in every path read off a window.
- A mark that would capture the whole desktop — a screenshot or design mark clicked
  without dragging — says so in the composer before it is sent.
- Screenshots are held in memory and never written to disk.
- No telemetry, no analytics, no crash reporting, no update check. Everything the toolbar
  sends goes to your own OpenClaw Gateway. The component library is the one exception: its
  preview pictures come from `cdn.21st.dev`, which therefore sees your IP address while
  that panel is open. No other host is accepted for a preview.

### Where it runs

- Linux, X11, glibc 2.35 and newer — Ubuntu 22.04, Debian 12, Fedora 38 and later.
- It refuses to start on Wayland rather than pointing at the wrong window, and says how to
  switch to an X11 session.
- It says which of `xprop` and `xwininfo` is missing, and how to install them, instead of
  silently failing to name the window a mark was made on.
- A missing runtime library surfaces as the loader's own message naming the library.
- The binary travels compressed and unpacks on first use, so the plugin's directory must
  be writable the first time the toolbar is asked for. What is unpacked is checked against
  the digest shipped beside it before it is ever run.

Proved on Ubuntu 22.04, Debian 12 and Debian 13, installed from the packed tarball into a
machine that had never seen it, drawn on a screen of that machine's own, and uninstalled
again leaving nothing behind. The Fedora and Arch dependency lines above were run in clean
containers of those distributions.
