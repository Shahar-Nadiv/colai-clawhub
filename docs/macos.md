# colai on macOS

colai started on Linux, then Windows. This is the macOS port: what it does, what nobody
has watched it do yet, and what is still to come before it can be handed to strangers.

The code is written and type-checked for `aarch64-apple-darwin` on every change (see
[Checking without a Mac](#checking-without-a-mac)), built and tested by CI on a real Mac, and
shipped prebuilt under `platforms/darwin-arm64/` (see [Shipping](#shipping)) — but CI never
sees the screen. Until
the checklist below has been walked on a real Mac, treat every macOS behaviour here as a
claim.

## What the port does

The toolbar is one full-screen, transparent, always-on-top Tauri window. Every platform
has to answer the same handful of questions Tauri does not answer for it, and on a Mac the
answers come from AppKit and CoreGraphics:

| What the toolbar needs | Linux | Windows | macOS |
| --- | --- | --- | --- |
| A transparent window | GTK RGBA visual | WebView2 | WKWebView, through Tauri's `macos-private-api` feature and `app.macOSPrivateApi` in `tauri.conf.json` |
| Clicks fall through everywhere it has drawn nothing | GTK input shape | a watcher hit-testing `GetCursorPos` and toggling ignore-cursor-events | the same watcher, reading the pointer from `CGEvent` |
| The application in front | AT-SPI / X11 | foreground window | `NSWorkspace` front application and `CGWindowListCopyWindowInfo` |
| The list of displays | GDK monitors | `EnumDisplayMonitors` | CoreGraphics displays (the overlay covers the primary display first) |
| A picture of the screen, with the mark drawn on it | gdk-pixbuf + cairo | GDI `BitBlt` + `tiny-skia` + `image` | CoreGraphics capture + the same `tiny-skia` + `image` code as Windows |
| File and folder pickers | GTK `FileChooserNative` | `rfd` | `rfd` (`NSOpenPanel`) |
| Is that process still alive | `/proc` | process handle | `kill(pid, 0)` |
| Which binary to run | `Linux-x86_64` | `MINGW*-x86_64` | `Darwin-arm64` → `darwin-arm64`, `Darwin-x86_64` → `darwin-x64`, Rosetta → `darwin-arm64` |

The launcher (`bin/colai-toolbar`) is the same POSIX shell script on every platform. It
already finds `shasum` where Linux has `sha256sum`, and caches under `~/.cache/colai/`.

## Permissions a Mac will ask for

macOS guards the screen and other applications behind TCC — the privacy prompts in
**System Settings → Privacy & Security**. The permission belongs to the program that asked,
and for a toolbar launched from Claude Code that is often the terminal (Terminal, iTerm,
VS Code) rather than `colai-toolbar` itself. Expect to grant:

- **Screen Recording** — for captures. Without it macOS does not fail the grab: it hands
  back the desktop wallpaper and the menu bar, with every other application's windows
  missing, and other applications' window titles come back empty. A capture that shows
  only wallpaper means this permission, not a bug. After granting it the toolbar has to be
  restarted; macOS does not apply it to a running process.
- **Accessibility** — only if a later feature reads or drives another application's
  controls. The pointer position, the front application and the window list do not need
  it.

Neither prompt has a usage-string key, so no `Info.plist` is needed for the bare binary.

## Testing it in a VM

A macOS VM on Apple Silicon (UTM, Parallels, Tart or VirtualBox 7.1+) runs arm64 macOS,
which is the build that matters first. Note that a VM's virtual display has a scale factor
of its own; test once at 1x and, if the VM allows it, once at 2x (Retina), because points
and pixels only disagree at 2x.

### Build

```sh
xcode-select --install                  # Apple's compiler and linker
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
git clone <this repository> && cd colai-clawhub
npm install
npm run check:macos                     # the type-check, natively this time
(cd toolbar/src-tauri && cargo test)    # the Rust suite
npm test                                # the JS suite, including the launcher on Darwin
npm run build:toolbar                   # bin/colai-toolbar.built
```

### Checklist

Run the toolbar with `bin/colai-toolbar show` (it prefers the build you just made), then:

1. **It appears.** The toolbar is drawn over the desktop, the rest of the window is fully
   transparent — no grey or white sheet — and it stays above other windows, including when
   another application is brought to the front.
2. **Clicks fall through.** Click, scroll and drag on a window *behind* the overlay where
   colai has drawn nothing: the window behind gets them. Then click the toolbar: colai gets
   them. Move between the two quickly; neither should be sticky.
3. **Retina.** On a 2x display, the toolbar's hit areas line up with what is drawn — no
   clicks landing a few hundred pixels off, no half-sized or double-sized marks.
4. **The front window.** Bring a few applications forward in turn (Safari, Finder,
   TextEdit); colai follows which one is in front and does not count itself.
5. **Capture, before permission.** Take a capture. macOS should prompt for Screen
   Recording, or the capture comes back as wallpaper only. Note which application the
   prompt names.
6. **Capture, after permission.** Grant it, restart the toolbar, capture again: the picture
   shows the real windows, and the box, arrow or stroke is drawn in the right place, at the
   right size, on a 1x and a 2x display.
7. **Displays.** With a second display attached (or a second virtual one), the overlay
   covers the primary display and nothing crashes when a display is added or removed.
8. **Pickers.** Attach a file and a folder: the macOS open panel appears *in front of* the
   overlay, not hidden behind it, and the chosen path arrives.
9. **Global shortcut.** The toolbar's shortcut works with another application focused.
10. **Hand-off.** Send something to Claude Code and see it arrive with its picture; quit
    the toolbar and confirm a new Claude Code session's SessionStart hook brings it back,
    and that a second launch does not open a second toolbar.
11. **The shipped path.** `npm run stage:macos`, delete `bin/colai-toolbar.built`, run
    `bin/colai-toolbar show` again: it unpacks into `~/.cache/colai/<digest>/` and runs.

Write down anything that misbehaves with the macOS version, the VM, and the display scale.

## Checking without a Mac

`npm run check:macos` type-checks every `#[cfg(target_os = "macos")]` arm from Windows or
Linux. It needs only the target's standard library:

```sh
rustup target add aarch64-apple-darwin
npm run check:macos                     # or: node scripts/check-macos.mjs --x64
```

Nothing is compiled to machine code or linked. Off a Mac the script writes a stand-in C
compiler and archiver to the temp directory, because one dependency
(`objc2-exception-helper`) compiles a few lines of Objective-C in its build script and there
is no Apple clang to give them to. CI runs this on the Windows job, and builds and tests
natively on a `macos-latest` (Apple Silicon) runner.

## Shipping

No Mac is needed: the **Build macOS toolbar** workflow (`.github/workflows/build-macos.yml`,
run from the Actions tab) builds the Apple Silicon toolbar on a GitHub macOS runner, ad-hoc
signs it, verifies it, and commits it to `platforms/darwin-arm64/bin/` so every Mac install
picks it up automatically. It is the same as doing it by hand on a Mac:

`npm run stage:macos`, on a Mac, after `npm run build:toolbar`. It reads the CPU out of the
binary to pick `platforms/darwin-arm64/` or `platforms/darwin-x64/`, ad-hoc signs it (Apple
Silicon will not run unsigned arm64 code), and writes the `.gz`, `.sha256` and
`.build.json` the launcher reads, the same three files every platform ships.

## Later

- **Developer ID signing and notarization.** Ad-hoc signing satisfies the kernel, not
  Gatekeeper. That is enough today because a git clone and a shell-script unpack set no
  quarantine flag, so Gatekeeper never looks. A binary downloaded through a browser, or
  any future `.app`, would need `codesign --sign "Developer ID Application: …"
  --options runtime`, `xcrun notarytool submit --wait`, and `xcrun stapler staple`.
- **A stable TCC identity.** Permissions are keyed to the signing identity; an ad-hoc
  signature changes with every build, so macOS may ask again after each update. A Developer
  ID signature fixes that.
- **Every display, not just the primary.** The overlay covers the primary display first;
  spanning displays with different scale factors is a separate piece of work.
- **An Intel build.** The launcher and the stage script already know `darwin-x64`; nothing
  has been built or tested for it.
