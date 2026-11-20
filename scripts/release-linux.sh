#!/usr/bin/env bash
# Build Linux release packages of SOAP Client for this machine's architecture:
#   - a .deb for Debian 12+ and Ubuntu 22.04+, built in an Ubuntu 22.04 container (Docker)
#   - an Arch Linux package repackaged from that .deb (only when run on Arch Linux)
#
# Official releases are built for x86_64 and arm64 by .github/workflows/release-linux.yml;
# this script is for building and testing them locally.
#
# Requirements: Docker (your user in the `docker` group, or DOCKER="sudo docker"),
# and makepkg for the Arch package.
#
# Usage:  npm run release:linux
# Output: release/<version>/soap-client_<version>_<arch>.deb,
#         soap-client-bin-<version>-1-<arch>.pkg.tar.zst and SHA256SUMS-linux.txt
set -euo pipefail

cd "$(dirname "$0")/.."

say() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
die() { printf '\n\033[31mError:\033[0m %s\n' "$*" >&2; exit 1; }

DOCKER="${DOCKER:-docker}"
IMAGE="soap-client-linux-build"

# ---- preconditions ----
command -v git >/dev/null || die "git is required"
$DOCKER info >/dev/null 2>&1 || die "can't use Docker: start it, and add yourself to the docker group
  (sudo usermod -aG docker \"\$USER\", then log in again), or run with DOCKER=\"sudo docker\""

VERSION="$(node -p "require('./package.json').version")"
for f in src-tauri/tauri.conf.json src-tauri/Cargo.toml; do
  grep -Eq "\"?version\"? *[:=] *\"${VERSION}\"" "$f" || die "version in $f doesn't match package.json (${VERSION})"
done

# The build uses the committed source (git archive), never stray local files.
REV=HEAD
if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
  [[ "${ALLOW_DIRTY:-}" == "1" ]] || die "working tree has uncommitted changes (commit them, or set ALLOW_DIRTY=1)"
  REV="$(git stash create)"   # a commit of the tracked changes, without touching the working tree
  say "Building uncommitted changes to tracked files (untracked files are not included)"
fi

case "$(uname -m)" in
  x86_64) DEB_ARCH=amd64 ;;
  aarch64) DEB_ARCH=arm64 ;;
  *) die "unsupported architecture $(uname -m)" ;;
esac
OUT="release/${VERSION}"
DEB="${OUT}/soap-client_${VERSION}_${DEB_ARCH}.deb"
mkdir -p "$OUT"

# ---- .deb (Ubuntu 22.04 container) ----
say "Preparing the build image (Ubuntu 22.04)"
$DOCKER build -t "$IMAGE" packaging/linux

say "Building soap-client ${VERSION} (${DEB_ARCH}) .deb"
git archive --format=tar "$REV" | $DOCKER run --rm -i \
  -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
  -v "$(pwd)/${OUT}:/out" \
  -v soap-client-cargo-registry:/usr/local/cargo/registry \
  "$IMAGE" bash -c 'mkdir /build && tar -x -C /build && /build/packaging/linux/build-deb.sh /out'
[[ -f "$DEB" ]] || die "no .deb was produced"

say "Checking the .deb installs and finds its libraries on Debian 12 and Ubuntu 22.04"
for distro in debian:12 ubuntu:22.04; do
  $DOCKER run --rm -v "$(pwd)/${OUT}:/out:ro" "$distro" bash -c "
    set -e; export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq && apt-get install -y -qq /out/$(basename "$DEB") >/dev/null
    missing=\$(ldd /usr/bin/soap-client | grep 'not found' || true)
    [ -z \"\$missing\" ] || { echo \"\$missing\"; exit 1; }
    test -f /usr/share/applications/*.desktop
    echo '  $distro: OK'"
done

# ---- Arch Linux package ----
FILES=("$(basename "$DEB")")
if command -v makepkg >/dev/null; then
  say "Building the Arch Linux package"
  packaging/arch/build-pkg.sh "$DEB" "$OUT"
  PKG="soap-client-bin-${VERSION}-1-$(uname -m).pkg.tar.zst"
  [[ -f "${OUT}/${PKG}" ]] || die "no Arch package was produced"
  FILES+=("$PKG")
else
  say "Skipping the Arch Linux package (makepkg not found; it is built on Arch Linux only)"
fi

# ---- checksums ----
(cd "$OUT" && sha256sum "${FILES[@]}" > SHA256SUMS-linux.txt)

say "Done: ${OUT}/"
cat "${OUT}/SHA256SUMS-linux.txt"
cat <<EOF

Try it:
  sudo apt install ./${DEB}                        # Debian / Ubuntu
  sudo pacman -U ${OUT}/soap-client-bin-*.pkg.tar.zst   # Arch Linux
EOF
