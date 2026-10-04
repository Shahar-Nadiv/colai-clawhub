#!/bin/sh
# Build the shipping toolbar inside the release image, and stage it.
#
# The crate is copied to /build rather than mounted from the developer's checkout,
# because the path is part of the artifact: Tauri embeds its build context, and
# `--remap-path-prefix` cannot rewrite that one. Whatever this directory is called is
# what every user who installs the toolbar will find inside it.
set -eu

echo "colai: building the shipping toolbar on $(ldd --version | head -1)"

cd /build/src-tauri
cargo build --release --locked \
  --config "profile.release.strip='symbols'"

cp target/release/colai-toolbar /out/colai-toolbar
strip /out/colai-toolbar 2>/dev/null || true

# Cleared from in here, where root owns it. The container writes `target/` as root, and
# the host that staged this directory then cannot delete its own temporary tree — the
# build succeeds and the script fails on the way out.
rm -rf /build/src-tauri/target

echo "colai: built. What it needs to run:"
# The number that decides which machines this works on. Printed rather than assumed,
# because it is set by this image and would move silently if the image ever did.
readelf -V /out/colai-toolbar | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1
