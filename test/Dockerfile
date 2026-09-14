# A machine that has never seen colai.
#
# The point of this image is what it does NOT contain: no Rust, no GTK headers, no cargo
# cache, and no OpenClaw checkout. It installs a released `openclaw` from npm rather than
# the fork this plugin was cut from, because the toolbar carries a copied Gateway client
# and testing it only against its own source is exactly the blind spot that copy has.
#
# By default there is no display in here. This proves packaging, the pre-flight checks,
# install and uninstall, and — since the binary now refuses a machine with no screen rather
# than crashing on one — that the shipped binary actually loads and runs here.
# `fresh-install.sh --draws` starts Xvfb inside the container for the last mile.
#
# The base is an argument, and that is the whole reason this file changed. It was pinned to
# Debian 13, whose glibc is 2.41 — so the test could not fail on a binary built against a
# newer glibc than it promises, which is precisely the bug that shipped. The floor the
# package claims is 2.35, so the matrix has to include a machine that old.
ARG BASE=debian:trixie-slim
FROM ${BASE}

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates curl xz-utils git python3 \
  && rm -rf /var/lib/apt/lists/*

# Node from nodejs.org rather than apt, so the version is the one OpenClaw asks for and
# not whatever the distribution happens to carry.
ARG NODE_VERSION=22.23.2
RUN curl -fsSL "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
    | tar -xJ -C /usr/local --strip-components=1 \
  && node --version

# A released OpenClaw, pinned. Not the fork.
ARG OPENCLAW_VERSION=2026.9.1
RUN npm install -g "openclaw@${OPENCLAW_VERSION}" && openclaw --version

# What the shipped binary needs at *run* time — not the `-dev` packages, which are what it
# would need to be compiled. This is the set the README tells people to install, so getting
# it wrong here is getting the README wrong.
#
# `WITH_RUNTIME=0` is a variant of its own: a machine that has none of this must produce a
# loader error somebody can act on, rather than a window that never appears.
#
# `xvfb` is here so the `--draws` check has a screen that belongs to nobody. It is not part
# of what a user installs — `x11-utils` is, and is listed in the README — but a screen the
# container makes up is the only way to ask whether the toolbar draws without borrowing the
# developer's desktop, and borrowing it meant photographing it.
ARG WITH_RUNTIME=1
RUN if [ "$WITH_RUNTIME" = "1" ]; then \
      apt-get update \
      && apt-get install -y --no-install-recommends \
        libwebkit2gtk-4.1-0 libgtk-3-0 libsoup-3.0-0 libayatana-appindicator3-1 x11-utils \
        xvfb \
      && rm -rf /var/lib/apt/lists/*; \
    fi

# Kept so a variant can prove a toolchain it does not need changes nothing. Off by default
# now: the question "does this need Rust on the installing machine" is settled — it ships a
# binary — and the question that replaced it is "how old a Linux does that binary reach".
ARG WITH_RUST=0
ARG RUST_VERSION=1.97.1
RUN if [ "$WITH_RUST" = "1" ]; then \
      apt-get update && apt-get install -y --no-install-recommends build-essential pkg-config \
      && rm -rf /var/lib/apt/lists/* \
      && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --profile minimal --default-toolchain "$RUST_VERSION"; \
    fi
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /work
