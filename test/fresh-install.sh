#!/usr/bin/env bash
# Install colai the way somebody else would, on a machine that has never seen it.
#
# `openclaw plugins install -l <dir>` on this desktop proves the wiring and close to
# nothing else: there is a configured Gateway here, a Rust toolchain, every header, and a
# warm cargo cache that turns a multi-minute cold build into seconds — and `-l` links the
# working tree, so a file missing from `package.json` still resolves.
#
# So this packs the plugin the way npm would, hands the tarball to a released OpenClaw
# inside a container, and reports what happened. Four things it can catch that a local
# link cannot:
#
#   1. packaging      — only what `files` ships is there
#   2. the toolbar    — it ships already built, because OpenClaw installs plugins with
#                       `--ignore-scripts` and nothing can compile on the far machine
#   3. protocol drift — a released `openclaw`, not the fork this was cut from
#   4. uninstall      — nothing left behind
#
# Usage:
#   ./fresh-install.sh                    every variant
#   ./fresh-install.sh jammy              one variant
#   ./fresh-install.sh --draws jammy      start a screen inside it, so it can draw
#   ./fresh-install.sh --keep jammy       leave the image behind, for iterating
#
# Variants are distributions now, not toolchain combinations. `jammy` is the one that
# matters: Ubuntu 22.04 is glibc 2.35, which is exactly the floor the package claims, so a
# binary built one release too new passes everything else here and fails that.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
plugin="$(cd "$here/.." && pwd)"
# A directory of this run's own. Two runs sharing one path is a tarball replaced under a
# container that had already mounted it — which fails as `EISDIR` several steps later and
# reads like a packaging bug rather than what it is.
work="$(mktemp -d "${TMPDIR:-/tmp}/colai-fresh-install-XXXXXX")"
# Give the container a screen of its own and ask the X server what landed on it.
check_draws=0
# Images are removed as each variant finishes; `--keep` holds on to them for iterating.
keep_images=0
declare -a wanted=()

for arg in "$@"; do
  case "$arg" in
    --draws) check_draws=1 ;;
    --keep) keep_images=1 ;;
    -*) echo "unknown option: $arg" >&2; exit 2 ;;
    *) wanted+=("$arg") ;;
  esac
done

# name|BASE|WITH_RUNTIME|WITH_RUST|what it proves
#
# Pipe-separated, because a base image has a colon in it and the old colon-separated table
# could not hold `ubuntu:22.04`.
#
# These used to be about whether a toolchain was present, which was the right question
# when it was not yet certain the plugin could avoid compiling on the installing machine.
# It can — it ships a binary — so the question that replaced it is how old a Linux that
# binary reaches, and whether it says something useful where it cannot run at all.
#
# `jammy` is the one that matters. The package claims a glibc floor of 2.35 and Ubuntu
# 22.04 *is* 2.35, so it is the exact edge: a binary built one release too new passes every
# other variant here and fails this one. The old matrix could not fail on that, because
# every variant ran on Debian 13.
VARIANTS=(
  "jammy|ubuntu:22.04|1|0|the oldest Linux the package claims — glibc 2.35, the floor itself"
  "bookworm|debian:12|1|0|the other common LTS, and a different webkit build"
  "trixie|debian:trixie-slim|1|0|a current machine, with nothing but a released OpenClaw"
  "nolibs|debian:trixie-slim|0|0|a machine missing WebKitGTK says so in a way somebody can act on"
  "rust|debian:trixie-slim|1|1|a toolchain it does not need changes nothing"
)

if [ ${#wanted[@]} -eq 0 ]; then
  wanted=(jammy bookworm trixie nolibs)
fi

# Two things are built before anything is packed, because neither can happen on the
# installing machine: the plugin's runtime JavaScript, since an installed plugin loads
# compiled JS and only a source checkout gets the TypeScript fallback; and the toolbar
# itself, since OpenClaw passes `--ignore-scripts` to every managed npm install.
echo "building the plugin runtime"
(cd "$plugin" && node scripts/build-runtime.mjs)

echo "packing $plugin"
#
# The binary lives in a package of its own now — `@colai/toolbar-linux-x64` — which the
# wrapper names as an optional dependency and npm fetches from the registry. That is the
# thing this test most needs to exercise and the one thing it cannot: the platform package
# is not published yet, and a container with no network to a registry that does not have it
# would only ever prove the failure path.
#
# So both are packed, and the wrapper's dependency is pointed at the tarball beside it for
# the length of the run. `file:` is a spec npm resolves exactly like any other, so the
# install under test is the real one — same resolution, same lockfile, same layout on disk
# — with only the source of the bytes changed. The manifest is put back afterwards, always,
# including when this script is interrupted.
echo "packing the linux-x64 binary"
platform="$work/$( (cd "$plugin/platforms/linux-x64" && npm pack --pack-destination "$work" --silent) | tail -1 )"

manifest="$plugin/package.json"
cp "$manifest" "$work/package.json.real"
restore_manifest() { cp "$work/package.json.real" "$manifest" 2>/dev/null || true; }
trap restore_manifest EXIT INT TERM
node -e '
  const fs = require("node:fs");
  const [file, tarball] = process.argv.slice(1);
  const manifest = JSON.parse(fs.readFileSync(file, "utf8"));
  manifest.optionalDependencies["@colai/toolbar-linux-x64"] = `file:${tarball}`;
  fs.writeFileSync(file, `${JSON.stringify(manifest, null, 2)}\n`);
' "$manifest" "/work/$(basename "$platform")"

echo "packing $plugin"
tarball="$work/$( (cd "$plugin" && npm pack --pack-destination "$work" --silent) | tail -1 )"
restore_manifest
echo "packed $(basename "$tarball") ($(du -h "$tarball" | cut -f1))"
echo

declare -a results=()

for variant in "${wanted[@]}"; do
  spec=""
  for candidate in "${VARIANTS[@]}"; do
    [ "${candidate%%|*}" = "$variant" ] && spec="$candidate"
  done
  if [ -z "$spec" ]; then
    echo "unknown variant: $variant" >&2
    exit 2
  fi
  IFS='|' read -r name base with_runtime with_rust proves <<< "$spec"

  echo "=== $name — $proves"
  echo "    on $base, runtime libraries: $with_runtime"
  image="colai-fresh:$name"
  docker build \
    --build-arg "BASE=$base" \
    --build-arg "WITH_RUNTIME=$with_runtime" \
    --build-arg "WITH_RUST=$with_rust" \
    -t "$image" -f "$here/Dockerfile" "$here" >/dev/null

  # Which of the two outcomes above this variant is entitled to.
  declare -a wants=(-e "WANT_RUNTIME=$with_runtime")

  # The last mile, and the only check here that asks whether anything appears.
  #
  # Everything else proves the toolbar *refuses* correctly — no screen, no Wayland, no
  # libraries. None of it proves it draws, which is the entire product.
  #
  # The screen is made up inside the container. An earlier version passed this desktop's X
  # socket in and compared photographs of it before and after, which answered a question
  # about one window by capturing everything else on the developer's screen and writing it
  # to a temporary directory. A private screen and a direct question to the X server is
  # both less and better: a photograph can only say something changed, and the server can
  # say which window, how big, and whether it is actually mapped.
  #
  # Only where the runtime libraries are installed. A variant built without them is
  # supposed to fail in the loader, and that has nothing to do with drawing.
  declare -a screen=()
  if [ "$check_draws" = "1" ] && [ "$with_runtime" = "1" ]; then
    screen=(-e "WANT_DRAW=1")
  fi

  began=$(date +%s)
  if docker run --rm "${wants[@]}" "${screen[@]+"${screen[@]}"}" \
      -v "$tarball:/work/$(basename "$tarball"):ro" \
      -v "$platform:/work/$(basename "$platform"):ro" \
      "$image" bash -euo pipefail -c "
        # --accept-capabilities because nothing in here can answer a prompt. A person
        # installing this is shown the same surface and accepts it themselves.
        openclaw plugins install 'npm-pack:/work/$(basename "$tarball")' --force --accept-capabilities
        echo
        echo '--- what colai says for itself'
        # Not doctor: core names the five bundled plugins that contribute health checks
        # and has no seam for an installed one, so doctor never loads this plugin. Its
        # own command does.
        openclaw colai
        echo
        echo '--- is the toolbar there'
        binary=\$(find ~/.openclaw -type f -name colai-toolbar 2>/dev/null | head -1)
        if [ -z \"\$binary\" ]; then
          echo 'MISSING — no colai-toolbar anywhere under ~/.openclaw'
          exit 1
        fi
        echo \"there: \$binary (\$(du -h \"\$binary\" | cut -f1))\"
        #
        # One build, and the right one.
        #
        # The wrapper names every platform package as an optional dependency, and the
        # whole promise of that arrangement is that npm reads the \`os\` and \`cpu\` on each
        # and installs only the one this machine can run. If that promise ever broke, the
        # install would still succeed and still work here — it would just also carry a
        # Mac binary to every Linux user, silently, which is exactly the kind of thing
        # nobody notices until the tarball is twice the size it should be.
        installed=\$(ls ~/.openclaw/npm/projects/*/node_modules/@colai/ 2>/dev/null | grep '^toolbar-' || true)
        echo \"builds installed: \$(echo \$installed | tr '\\n' ' ')\"
        if echo \"\$installed\" | grep -q 'darwin'; then
          echo 'WRONG BUILD — a macOS package was installed on Linux'
          exit 1
        fi
        if ! echo \"\$installed\" | grep -q 'toolbar-linux-x64'; then
          echo 'MISSING BUILD — the linux-x64 package did not arrive'
          exit 1
        fi
        #
        # Shipped from another machine, so whether it can actually run here is the whole
        # question — and until now this did not ask it. It ran the binary, threw the exit
        # code away with \`|| true\`, threw the loader's message away with \`2>&1\`, and
        # then checked the first four bytes for \`ELF\`. A file that cannot load on this
        # machine at all still starts with ELF, so the check could not fail on the one
        # class of bug it exists to catch.
        #
        # It runs it now, and reads what came out. With no display — every variant but
        # \`--draws\` — a binary that loads must refuse with the sentence it promises, and
        # one that cannot load says so through the dynamic loader instead. With a screen,
        # refusing is the wrong answer and the question becomes what it put on it.
        #
        if [ \"\${WANT_DRAW:-0}\" = '1' ]; then
          # A screen of this container's own, thrown away with it.
          Xvfb :99 -screen 0 1920x1080x24 >/tmp/xvfb.log 2>&1 &
          for _ in \$(seq 1 20); do xdpyinfo -display :99 >/dev/null 2>&1 && break; sleep 1; done
          if ! xdpyinfo -display :99 >/dev/null 2>&1; then
            echo 'NO SCREEN — Xvfb never came up, so this proves nothing:'
            cat /tmp/xvfb.log
            exit 1
          fi
          # \`show\` runs the event loop and does not return, so it goes to the background.
          DISPLAY=:99 \"\$binary\" show --pidfile /tmp/colai.pid >/tmp/colai.log 2>&1 &
          drew=\$!
          sleep 8
          if ! kill -0 \$drew 2>/dev/null; then
            echo 'IT EXITED instead of drawing:'; cat /tmp/colai.log; exit 1
          fi
          #
          # Ask the server what is on it.
          #
          # \`-root -tree\` and not a window-manager query: there is no window manager on
          # this screen, and the overlay is override-redirect anyway, so it never appears
          # in \`_NET_CLIENT_LIST\`. The tree is the server's own record of every window.
          #
          # The title is \`colai\`, set in \`colai.rs\`. Nothing else has a client here.
          id=\$(DISPLAY=:99 xwininfo -root -tree 2>/dev/null \
               | grep -i '\"colai\"' | grep -oE '0x[0-9a-f]+' | head -1)
          if [ -z \"\$id\" ]; then
            echo 'NO WINDOW — it stayed alive with a screen and put nothing on it:'
            DISPLAY=:99 xwininfo -root -tree 2>/dev/null | head -20
            cat /tmp/colai.log
            kill \$drew 2>/dev/null || true
            exit 1
          fi
          # A window can exist and be unmapped, or be one pixel. Neither is a toolbar.
          about=\$(DISPLAY=:99 xwininfo -id \"\$id\" 2>/dev/null)
          kill \$drew 2>/dev/null || true
          if ! echo \"\$about\" | grep -q 'Map State: IsViewable'; then
            echo 'NOT VIEWABLE — the window exists but is not on the screen:'
            echo \"\$about\"
            exit 1
          fi
          wide=\$(echo \"\$about\" | awk '/Width:/{print \$2}')
          tall=\$(echo \"\$about\" | awk '/Height:/{print \$2}')
          if [ \"\${wide:-0}\" -lt 40 ] || [ \"\${tall:-0}\" -lt 40 ]; then
            echo \"TOO SMALL — \${wide}x\${tall} is not a toolbar:\"
            echo \"\$about\"
            exit 1
          fi
          #
          # What this does not prove: what WebKit painted inside that window. The X server
          # knows the window and nothing about its contents, and the screenshot this
          # replaced did not prove it either — it compared the whole desktop, so a moved
          # cursor passed it. Painting is the toolbar's own test suite's job.
          echo \"and it draws: \$id, \${wide}x\${tall}, mapped and viewable\"
          said='drew'
        else
          said=\$(\"\$binary\" show 2>&1 </dev/null; true)
          echo \"said: \$said\"
          if echo \"\$said\" | grep -q 'error while loading shared libraries'; then
            if [ \"\$WANT_RUNTIME\" = '1' ]; then
              echo 'CANNOT LOAD — the runtime libraries are installed and it still will not start'
              exit 1
            fi
            # The variant with no runtime libraries. What matters is that the failure names
            # a library — that name is the only thing telling somebody what to install.
            #
            # Which library it names is not fixed and must not be asserted: the loader
            # stops at the first one it cannot find, and on a base image with nothing
            # installed that is \`libX11.so.6\` long before it ever reaches WebKitGTK.
            if ! echo \"\$said\" | grep -qE 'lib[A-Za-z0-9_.+-]*\\.so'; then
              echo 'the loader failed without naming what is missing'
              exit 1
            fi
            echo \"and where it cannot load, it names what is missing: \$(echo \"\$said\" | grep -oE 'lib[A-Za-z0-9_.+-]*\\.so[0-9.]*' | head -1)\"
          elif echo \"\$said\" | grep -q 'no screen to draw on'; then
            if [ \"\$WANT_RUNTIME\" != '1' ]; then
              echo 'it started without the libraries it is supposed to need'
              exit 1
            fi
            echo 'and it runs here: loaded, started, and refused a machine with no screen'
          else
            echo 'UNEXPECTED — it neither ran nor failed in a way this test understands'
            exit 1
          fi
        fi
        echo
        echo '--- uninstall'
        openclaw plugins uninstall colai --force
        left=\$(find ~/.openclaw -name '*colai*' 2>/dev/null | head)
        if [ -n \"\$left\" ]; then echo 'LEFT BEHIND:'; echo \"\$left\"; exit 1; fi
        echo 'nothing left behind'
      "; then
    took=$(( $(date +%s) - began ))
    results+=("$name|ok|${took}s|$proves")
  else
    took=$(( $(date +%s) - began ))
    results+=("$name|FAILED|${took}s|$proves")
  fi

  # Removed here rather than at the end, which is what the cleanup below used to do.
  #
  # Each of these images is about 2.5 GB — a base, Node, a released OpenClaw and the
  # WebKitGTK runtime — and keeping every variant's image until the last one finished
  # meant the default set needed ten gigabytes free before it could complete. It did not
  # have them, and the run died partway through the third variant with no result and no
  # explanation. One at a time costs a rebuild if you re-run a variant, and a rebuild is
  # cheaper than a matrix that cannot finish.
  #
  # `--keep` opts out, for anyone iterating on one variant.
  if [ "$keep_images" != "1" ]; then
    docker rmi -f "$image" >/dev/null 2>&1 || true
  fi
  echo
done

echo "=================================================================="
printf '%-12s %-8s %-8s %s\n' variant result took proves
for row in "${results[@]}"; do
  IFS='|' read -r name result took proves <<< "$row"
  printf '%-12s %-8s %-8s %s\n' "$name" "$result" "$took" "$proves"
done
echo "=================================================================="
echo
if [ "$check_draws" != "1" ]; then
  echo "None of this proves the overlay draws: run one variant with --draws for that."
fi

# The images are gigabytes each and exist for one run. Kept only when something failed,
# because that is when somebody wants to go and look inside one.
failed=0
for row in "${results[@]}"; do
  case "$row" in *"|FAILED|"*) failed=1 ;; esac
done
if [ "$failed" = "0" ]; then
  rm -rf "$work"
  for variant in "${wanted[@]}"; do
    docker rmi -f "colai-fresh:$variant" >/dev/null 2>&1 || true
  done
  docker builder prune -af >/dev/null 2>&1 || true
else
  echo "Left behind for inspection: $work, and the colai-fresh:* images."
  exit 1
fi
