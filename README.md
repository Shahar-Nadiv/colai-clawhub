# colai toolbar

Point at anything on your screen and hand it to an agent.

A small rail that floats over every application on your desktop. Draw a box round a thing,
point at a thing, measure it, pick a colour off it, record a few seconds of it — then say
what you want in a sentence and send it to an OpenClaw agent, along with a picture of
exactly what you meant.

It belongs to no application. There is no plugin to install in your editor, no browser
extension, no SDK. If it is on the screen, you can point at it — a canvas game, a PCB in a
3D viewer, a native desktop app, a PDF, a video call.

> **Linux and X11 only.** See [Requirements](#requirements) before installing. It refuses
> to start on Wayland rather than working badly, and it tells you how to switch.

## Install

```bash
openclaw plugins install @colai/toolbar
```

The toolbar ships already built, so nothing compiles on your machine and no Rust toolchain
is needed. It starts with the Gateway and puts itself on screen.

```bash
openclaw colai show      # put it on screen
openclaw colai hide      # put it away
openclaw colai toggle    # either
```

Or press <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Space</kbd> from anywhere. Set `COLAI_HOTKEY`
to change it.

## Requirements

|                    |                                                                                              |
| ------------------ | -------------------------------------------------------------------------------------------- |
| **Display server** | **X11.** Not Wayland — see below.                                                            |
| **OS**             | Linux, x86-64. glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 38, and anything later). |
| **Libraries**      | WebKitGTK 4.1, libsoup 3, GTK 3                                                              |
| **Tools**          | `xprop` and `xwininfo`, from `x11-utils`                                                     |
| **OpenClaw**       | 2026.9.1 or newer, with a model provider already configured                                  |

```bash
# Debian / Ubuntu
sudo apt install libwebkit2gtk-4.1-0 libsoup-3.0-0 libgtk-3-0 x11-utils

# Fedora
sudo dnf install webkit2gtk4.1 libsoup3 gtk3 xorg-x11-utils

# Arch
sudo pacman -S webkit2gtk-4.1 libsoup3 gtk3 xorg-xprop xorg-xwininfo
```

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
  pointed at. Pictures are held in memory and never written to disk.
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
check. The toolbar's only network connection is to your own OpenClaw Gateway.

## Where things live

|                               |                                                                   |
| ----------------------------- | ----------------------------------------------------------------- |
| Device key                    | `~/.config/ai.colai.toolbar/quickchat-gateway-device.json` (0600) |
| Rail position, recent prompts | `~/.local/share/ai.colai.toolbar/`                                |
| Log                           | `<openclaw state dir>/logs/colai-toolbar.log`                     |

`openclaw plugins uninstall @colai/toolbar` removes the plugin; the two directories above
are yours to delete.

## Troubleshooting

**Nothing appears.** Check the log. The most common causes are a Wayland session, a missing
`libwebkit2gtk-4.1`, and a Gateway that is not running.

**Marks do not say which window they were made on.** `x11-utils` is not installed — the
toolbar says so on startup, in the log.

**No tray icon.** Some GNOME sessions ship no AppIndicator extension. Use
`openclaw colai toggle` or the hotkey.

## Licence

MIT. The bundled fonts — Instrument Sans and JetBrains Mono — are under the SIL Open Font
Licence; their licences ship beside them in `toolbar/ui/fonts/`.
