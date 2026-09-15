# Changelog

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
