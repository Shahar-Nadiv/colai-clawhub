# The image the shipped toolbar is built in.
#
# Two things about a binary are decided by the machine that compiles it rather than by
# any of its source, and both were wrong when this was built on a developer's desk.
#
# **How old a Linux it will run on.** The binary records the highest glibc symbol version
# it needs, and the loader refuses — before `main`, with a message the toolbar never gets
# to explain — on anything older. Built on Ubuntu 24.04 that floor was GLIBC_2.39, which
# is Ubuntu 24.04 and almost nothing else: not 22.04, not Debian 12, not RHEL 9, not any
# musl distro. The floor is always exactly the build host's glibc, so the build host is
# the decision. Jammy is glibc 2.35, and it is the oldest release still carrying
# webkit2gtk-4.1 and libsoup-3.0, which the crate needs.
#
# **Whose computer it was built on.** Tauri embeds its build context path in the binary,
# and `--remap-path-prefix` cannot reach that one: it rewrites what rustc emits, and this
# is not that. The author's home directory therefore shipped inside every copy. Here the
# context is `/build`, which says nothing about anybody.
# **Which Ubuntu 22.04.** A tag is a moving pointer: `ubuntu:22.04` is rebuilt whenever its
# contents change, so two builds a month apart start from different bytes and no third
# party — including whoever wrote this — can rebuild a release and compare it with what was
# published. The Rust toolchain below was already pinned for exactly that reason; the base
# it runs on was not, which made the pin below a statement about one ingredient of a recipe
# whose others were "whatever is in the shop".
#
# Pinned to the digest that built the shipped 0.1.0 binary. A digest names bytes, not a
# moving target, and `22.04` is kept beside it because a bare digest tells a reader nothing
# about what it is — Docker verifies the digest and ignores the tag.
#
# Moving it is a deliberate act: change the digest, rebuild, and expect the binary to
# change. `docker pull ubuntu:22.04` prints the current one.
#
# This is necessary for a reproducible build and not sufficient: the apt packages installed
# below still float, so a rebuild months from now gets whatever `apt-get` serves that day
# and the binary will differ. Pinning those too is the next step, and it is a larger one —
# every version has to be found and they expire from the archive.
FROM ubuntu:22.04@sha256:829f6df217bcbae2b371026e81711d1a787c61b2967ad09d015063663ebafbf7

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential curl ca-certificates pkg-config file \
      libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
      libayatana-appindicator3-dev librsvg2-dev \
    && rm -rf /var/lib/apt/lists/*

# Pinned, because "whatever was current" is not a thing a release can be rebuilt from.
ARG RUST_VERSION=1.97.1
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain ${RUST_VERSION} --profile minimal
ENV PATH=/root/.cargo/bin:$PATH

WORKDIR /build
