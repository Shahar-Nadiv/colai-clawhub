# Changelog

## 0.1.0 — unreleased

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
- No telemetry, no analytics, no crash reporting, no update check. The only outbound
  connection is to your own OpenClaw Gateway.

### Where it runs

- Linux, X11, glibc 2.35 and newer — Ubuntu 22.04, Debian 12, Fedora 38 and later.
- It refuses to start on Wayland rather than pointing at the wrong window, and says how to
  switch to an X11 session.
- It says which of `xprop` and `xwininfo` is missing, and how to install them, instead of
  silently failing to name the window a mark was made on.
- A missing runtime library surfaces as the loader's own message naming the library.

Proved on Ubuntu 22.04, Debian 12 and Debian 13, installed from the packed tarball into a
machine that had never seen it, and uninstalled again leaving nothing behind.
