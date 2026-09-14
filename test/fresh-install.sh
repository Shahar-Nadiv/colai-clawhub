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
#   ./fresh-install.sh --display jammy    pass this desktop's screen in, so it can draw
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
share_display=0
# Images are removed as each variant finishes; `--keep` holds on to them for iterating.
keep_images=0
declare -a wanted=()

for arg in "$@"; do
  case "$arg" in
    --display) share_display=1 ;;
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

# One tarball for every variant: exactly what `npm publish` would upload, and the only
# thing any container is allowed to see of this repo.
echo "packing $plugin"
tarball="$work/$( (cd "$plugin" && npm pack --pack-destination "$work" --silent) | tail -1 )"
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

  declare -a screen=()
  if [ "$share_display" = "1" ] && [ -n "${DISPLAY:-}" ]; then
    # The last mile: a toolbar compiled inside the container, drawn on this desktop.
    screen=(-e "DISPLAY=$DISPLAY" -v /tmp/.X11-unix:/tmp/.X11-unix)
  fi

  began=$(date +%s)
  if docker run --rm "${wants[@]}" "${screen[@]+"${screen[@]}"}" \
      -v "$tarball:/work/$(basename "$tarball"):ro" \
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
        # Shipped from another machine, so whether it can actually run here is the whole
        # question — and until now this did not ask it. It ran the binary, threw the exit
        # code away with \`|| true\`, threw the loader's message away with \`2>&1\`, and
        # then checked the first four bytes for \`ELF\`. A file that cannot load on this
        # machine at all still starts with ELF, so the check could not fail on the one
        # class of bug it exists to catch.
        #
        # It runs it now, and reads what came out. There is no display in here, so a
        # binary that loads must refuse with the sentence it promises; one that cannot
        # load says so through the dynamic loader instead.
        #
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
echo "A container has no screen, so none of this proves the overlay draws."
echo "Run one variant with --display for that, at least once."

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
