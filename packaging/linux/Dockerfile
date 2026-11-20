# Build environment for the Linux .deb release.
#
# Ubuntu 22.04 has the oldest glibc (2.35) among the distributions we target, so a binary
# built here also runs on Debian 12+, Ubuntu 22.04+ and current Arch Linux.
# Used by scripts/release-linux.sh and .github/workflows/release-linux.yml.
FROM ubuntu:22.04

ARG NODE_MAJOR=26
ARG RUST_TOOLCHAIN=stable
ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH

RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential ca-certificates curl file pkg-config xz-utils \
      libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libdbus-1-dev \
    && rm -rf /var/lib/apt/lists/*

# Node.js from nodejs.org, verified against the published checksums.
RUN set -eu; \
    case "$(dpkg --print-architecture)" in amd64) a=x64 ;; arm64) a=arm64 ;; *) exit 1 ;; esac; \
    base="https://nodejs.org/dist/latest-v${NODE_MAJOR}.x"; \
    cd /tmp; curl -fsSLO "$base/SHASUMS256.txt"; \
    f="$(grep -o "node-v[0-9.]*-linux-${a}\.tar\.xz" SHASUMS256.txt | head -1)"; \
    curl -fsSLO "$base/$f"; grep " $f\$" SHASUMS256.txt | sha256sum -c -; \
    tar -xJf "$f" -C /usr/local --strip-components=1 --exclude='*.md' --exclude=LICENSE; \
    rm -f /tmp/*; node --version

RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain "$RUST_TOOLCHAIN" \
    && rustc --version
