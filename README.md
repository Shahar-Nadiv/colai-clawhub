<img src="media/brand/colai-jellyfish-red.svg" alt="" width="72" align="left">

# colai toolbar

Point at anything on your screen and hand it to Claude Code.

[![CI](https://github.com/Shahar-Nadiv/colai-clawhub/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/Shahar-Nadiv/colai-clawhub/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Shahar-Nadiv/colai-clawhub?sort=semver)](https://github.com/Shahar-Nadiv/colai-clawhub/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Platforms](https://img.shields.io/badge/platforms-Windows%20%7C%20Linux%20%7C%20macOS%20(preview)-555)

<img src="media/toolbar.png" alt="The colai rail over a desktop, with the drawing tools open" width="620">

[**Watch it in 30 seconds**](media/toolbar.mp4)

A small rail that floats over every application on your desktop. Draw a box round a thing,
point at a thing, measure it, pick a colour off it, record a few seconds of it — then say
what you want in a sentence and send it to Claude Code, along with a picture of exactly
what you meant.

It belongs to no application. There is no plugin to install in your editor, no browser
extension, no SDK. If it is on the screen, you can point at it — a canvas game, a PCB in a
3D viewer, a native desktop app, a PDF, a video call.

> **Windows (x86-64) and Linux/X11 today. macOS (Apple Silicon) is in preview:** it is built,
> signed and tested in CI on a real Mac and installs like the others, but nobody has walked
> its checklist by hand yet — see [docs/macos.md](docs/macos.md).
> See [Requirements](#requirements) before installing. On Linux it refuses to start on Wayland
> rather than working badly, and it tells you how to switch.

## Install

```bash
claude plugin marketplace add Shahar-Nadiv/colai-clawhub
claude plugin install colai@colai
```

The same two lines work on Windows, Linux and macOS, and colai works the same from a terminal
or from the Claude desktop app's Code tab. On Windows they run inside Git for Windows, which
Claude Code already requires, so nothing extra is needed to install.

**Using the desktop app?** colai does its work through the `claude` command, and the desktop
app keeps its own sign-in that `claude` cannot use. Sign the command in once with
`claude auth login`. Until then a mark still reaches your chat, but only with your next message.

That is the whole setup. **The toolbar then starts itself** the next time you open a
Claude Code session — there is no command to run. It is a desktop overlay, not a subprocess
of that conversation, so closing the session leaves it on screen.
<kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Space</kbd> shows it and hides it again, from anywhere,
including while you are typing in Claude Code; set `COLAI_HOTKEY` to change the shortcut, and
double-click the grip to put it away. Set `COLAI_AUTOSTART=0` if you would rather it not
start on its own (then run `launcher/colai-toolbar show` from the plugin's folder (`~/.claude/plugins/cache/colai/colai/<version>/`) to bring it up).

It opens pointed at the conversation it started in, so the first thing you mark already has
somewhere to go — and you can hand a mark to any other conversation from the toolbar's own
conversation menu.

**A mark you send goes into that chat.** Not into a second window and not into a copy of
the conversation — into the session you are sitting in, while you are sitting in it, in a
terminal or in the Claude desktop app's Code tab.

The plugin carries a small bridge that runs inside each session (a Claude Code hooks module,
`hooks/bridge.ts`). It puts your mark in through the session's own front door, as your own
message: straight away when the chat is idle, right after the current turn when it is busy.
No second agent, no extra cost, and nothing for you to type.

Where the bridge is not running, colai falls back to Claude Code's `SendMessage`: a small
agent passes the mark along, about 2¢ a mark on your own account, and it shows up as a
message from another session. If the chat cannot be reached at all, the mark waits on disk
and arrives with your next message instead.

colai starts from a `SessionStart` hook — a line Claude Code runs when a session begins,
before anything reaches the model — so the toolbar is simply there, with no command to
remember and nothing for Claude to do but mention it. To stop it entirely rather than hide
it, press the × at the end of the rail.

It talks to the `claude` you are already signed in to. There is no API key to paste, no
second account, and no service to run — it starts a Claude Code session of its own behind
the rail and streams the replies back into the Work panel.

**Every mark costs tokens on your own Claude account**, the same as anything else you ask
Claude Code. The rail shows what the conversation has cost so far.

The toolbar ships already built, so nothing compiles on your machine and no Rust toolchain
is needed. Today there are three builds, **Windows on x86-64**, **Linux on x86-64** and
**macOS on Apple Silicon** (preview, [docs/macos.md](docs/macos.md)), and any other machine
is told so by name rather than left with a broken install. It travels compressed, unpacks itself into a per-user cache the first time you ask
for it, and is checked against the digest shipped beside it before it is ever run.

## Requirements

|                    |                                                                                              |
| ------------------ | -------------------------------------------------------------------------------------------- |
| **OS**             | Windows 10 or 11, x86-64 — or Linux, x86-64 (glibc 2.35 or newer: Ubuntu 22.04, Debian 12, Fedora 38, and anything later). macOS 11+ on Apple Silicon in preview. |
| **Display server** | Linux only: **X11.** Not Wayland — see below. (Windows has no equivalent.)                   |
| **Libraries**      | Linux: WebKitGTK 4.1, libsoup 3, GTK 3. Windows: the Microsoft Edge WebView2 runtime — preinstalled on Windows 11; on older Windows 10 install it from Microsoft if the overlay doesn't render. |
| **Tools**          | Linux: `xprop` and `xwininfo`, from `x11-utils`                                              |
| **Claude Code**    | The `claude` command signed in (`claude auth login`), even if you work in the desktop app. Anything colai sends is billed to that account. |

```bash
# Debian / Ubuntu
sudo apt install libwebkit2gtk-4.1-0 libsoup-3.0-0 libgtk-3-0 x11-utils

# Fedora
sudo dnf install webkit2gtk4.1 libsoup3 gtk3 xprop xwininfo

# Arch
sudo pacman -S webkit2gtk-4.1 libsoup3 gtk3 xorg-xprop xorg-xwininfo
```

Each of those three lines has been run in a clean container of that distribution and
checked for the four libraries and the two programs it is supposed to provide. Fedora used
to say `xorg-x11-utils` here, which no longer exists — Fedora split it, and `xprop` and
`xwininfo` are now packages of their own.

On Windows there is nothing to install for the overlay itself: the toolbar needs the
Microsoft Edge WebView2 runtime, which ships with Windows 11. On an older Windows 10 that
has not had it installed, grab it from Microsoft if the overlay doesn't render.

### Why not Wayland

The toolbar works by asking X which window is in front, where it is, and what it is called.
Under Wayland those questions are answered by XWayland, and XWayland answers them only
about its own clients — a native Wayland window is not in the answer. The toolbar would
start, draw, let you mark something, and then tell the agent it was about a different
window entirely.

A tool that refuses is one you can work around. A tool that is confidently wrong about what
it photographed costs you the conversation you were trying to have. So it refuses, and says
so.

To get an X11 session: log out, and at the login screen click the gear beside the **Sign
In** button and choose **Ubuntu on Xorg** (or your desktop's equivalent).

## What a mark sends

This matters, so it is written down rather than left to be discovered.

When you send a mark, the agent receives:

- **A picture** of what you marked — the region you drew, or a little around the point you
  pointed at. A mark sent to a chat you have open is written to `.colai/marks/` in that
  project, because a chat can only read pictures from files; nothing is sent anywhere else.
- **What you typed**, if you typed anything.
- **The address of the window you marked on**: which application, its working directory,
  the document it has open, its title, and its size.

Two things are removed from that address before it leaves your machine:

- **URL query strings and fragments.** `https://app.example.com/orders?token=…` is sent as
  `https://app.example.com/orders?…`. The query is where session tokens, signed-link
  signatures and email addresses live, and the host and path are what identify the page.
- **Your account name.** Any `/home/<someone>` becomes `~`.

What is _not_ removed: the rest of the path. A project or client name in a directory name
will go, because that is how the agent knows which project you mean.

If a mark would capture your **whole desktop** — which is what a screenshot or design mark
means if you click without dragging — the composer says so before you send it.

Nothing else leaves. There is no telemetry, no analytics, no crash reporting and no update
check. Everything the toolbar sends goes to the Claude Code you are already signed in to,
on your own machine — colai runs no server and holds no account of its own.

The one exception is the component library: opening it loads preview pictures from `cdn.21st.dev`, so that host sees your IP address while the panel is open. Nothing about your screen or your prompt goes with them, and the toolbar refuses a preview from any other host.

## Where things live

|                               |                                      |
| ----------------------------- | ------------------------------------ |
| Rail position, recent prompts | `~/.local/share/ai.colai.toolbar/`   |
| Which toolbar is running      | `~/.config/ai.colai.toolbar/`        |
| The unpacked toolbar          | `~/.cache/colai/`                    |

Conversations are Claude Code's own, in `~/.claude/`, and colai neither adds to that nor
keeps a second copy.

`claude plugin uninstall colai` removes the plugin; the three directories above are yours
to delete.

## What the plugin runs, reads, writes and sends

Everything colai does, by the piece that does it.

**The skill** (`skills/marked-screenshots/SKILL.md`) is instructions and nothing else: how to read
a screenshot somebody has drawn on — a circle, box, arrow, highlight or pin, Colai's or any other
tool's — and answer about what is marked. It runs nothing and reads nothing. It is the part of
colai that also works in Claude's apps (web, desktop and mobile chat), where a plugin can't run
anything on your computer, so the toolbar itself is Claude Code's.

**The `SessionStart` hook** (`hooks/say-colai-is-here.sh`) reads the session id from the hook's
input, checks whether the toolbar is already running (its PID file in
`~/.config/ai.colai.toolbar/`), and if not starts it with `launcher/colai-toolbar show --in <session>`.
It prints one status line, the `🪼 Colai is running` message you see at the start of a session.
Set `COLAI_AUTOSTART=0` to stop it starting the toolbar.

**The `UserPromptSubmit` hook** (`hooks/take-what-colai-staged.sh`) looks in
`~/.config/ai.colai.toolbar/outbox/<this session>/` for marks you sent to this conversation. It
adds the text of each one (`say.txt`, which the toolbar wrote) to the prompt you are submitting,
then deletes that mark. It reads nothing else and sends nothing.

**The mod** (`hooks/bridge.ts`) runs inside each Claude Code session. Every second and a half it:

- writes the current time to `~/.config/ai.colai.toolbar/outbox/<session>/.bridge`, so the toolbar
  knows a session is listening;
- reads any mark waiting in that folder and submits its text (`say.txt`) to the session as your
  own prompt — the words you typed in the toolbar, a description of what you pointed at, and the
  paths of the pictures it saved — first writing a `delivered` file beside it, so no mark is ever
  submitted twice;
- answers the toolbar's questions about your artifacts: for a question file in `.ask/`, it calls
  Claude's own `Artifact` tool with `action: "list"` (and the artifact type, when one is asked
  for), and writes the listing back as `<id>.answer.json`. That is the only tool it calls.

It reads `XDG_CONFIG_HOME`, `HOME` or `USERPROFILE` to find that folder, and nothing else from
your environment.

**The toolbar** is the program in `platforms/<system>/bin/`, unpacked on first run into
`~/.cache/colai/` after its SHA-256 is checked against the `.sha256` file beside it. Its source is
`toolbar/` in this repository. It:

- captures the part of your screen you mark, and saves pictures for a chat to read in
  `.colai/marks/` of that project;
- reads which window is in front, and Claude Code's own session list and transcripts in
  `~/.claude/` (to show your conversations and mirror their replies), never writing there;
- runs the `claude` command you already have: to start a conversation you send a mark to, to hand
  a mark to one already open, and to run a scheduled task you set up when it comes due;
- opens pages in your browser: claude.ai pages from the Artifacts menu and Claude Code's
  scheduled-tasks page, and brings Claude Desktop forward;
- writes its own settings and state to the folders in the table above, and a short log of
  problems to `~/.config/ai.colai.toolbar/colai.log`.

It sends nothing over the network itself, apart from loading preview pictures from
`cdn.21st.dev` when you open the component library (see [What a mark sends](#what-a-mark-sends)).

## Troubleshooting

**Nothing appears.** Run `launcher/colai-toolbar show` from the plugin's folder (`~/.claude/plugins/cache/colai/colai/<version>/`) in a terminal and read what it says — the two
usual causes, a Wayland session and a missing `libwebkit2gtk-4.1`, both name themselves
there. Started from a snap-packaged terminal or editor, the toolbar drops that snap's
library paths and restarts itself; the line saying so is expected.

**Marks do not say which window they were made on.** `x11-utils` is not installed — the
toolbar says so on startup, in the log.

**The hotkey does nothing.** Something else on the desktop holds it — the toolbar says so
on startup, naming the chord. Set `COLAI_HOTKEY` to a free one and restart it. Running
the launcher with `show` always works regardless: a second launch hands its argument to the
copy already on screen.

## Glossary

- **Gateway** — the service an OpenClaw agent reported its activity to; this plugin talks to
  Claude Code directly and has no equivalent.
- **OpenClaw** — the prior project this toolbar grew out of, which some older code comments
  still reference.

## Licence

MIT. The bundled fonts — Instrument Sans and JetBrains Mono — are under the SIL Open Font
Licence; their licences ship beside them in `toolbar/ui/fonts/`.
