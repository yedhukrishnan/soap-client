#!/usr/bin/env bash
# Build the Arch Linux package from a release .deb. Must run on Arch Linux (makepkg), as a
# regular user. Writes soap-client-bin-<version>-1-<arch>.pkg.tar.zst to OUT_DIR.
#
# Usage: packaging/arch/build-pkg.sh <soap-client_VERSION_ARCH.deb> <out-dir>
set -euo pipefail

DEB="$(realpath "${1:?usage: build-pkg.sh <deb> <out-dir>}")"
OUT_DIR="${2:?usage: build-pkg.sh <deb> <out-dir>}"
mkdir -p "$OUT_DIR"
OUT_DIR="$(realpath "$OUT_DIR")"
HERE="$(cd "$(dirname "$0")" && pwd)"

base="$(basename "$DEB" .deb)"                       # soap-client_0.2.0_amd64
[[ "$base" =~ ^soap-client_([0-9][^_]*)_(amd64|arm64)$ ]] || { echo "unexpected .deb name: $base" >&2; exit 1; }
VERSION="${BASH_REMATCH[1]}"
DEB_ARCH="${BASH_REMATCH[2]}"
case "$DEB_ARCH" in amd64) CARCH=x86_64 ;; arm64) CARCH=aarch64 ;; esac
[[ "$(uname -m)" == "$CARCH" ]] || { echo "$DEB is for $CARCH but this machine is $(uname -m)" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cp "$DEB" "$WORK/${base}.deb"
SUM="$(sha256sum "$DEB" | cut -d' ' -f1)"
sed -e "s/^pkgver=.*/pkgver=${VERSION}/" \
    -e "s/^sha256sums_${CARCH}=.*/sha256sums_${CARCH}=('${SUM}')/" \
    "$HERE/PKGBUILD" > "$WORK/PKGBUILD"

# -d: dependencies are only needed at install time; this just repackages the binary.
(cd "$WORK" && PKGDEST="$OUT_DIR" PKGEXT=.pkg.tar.zst makepkg --force --nodeps --noconfirm)
ls -l "$OUT_DIR"/soap-client-bin-"${VERSION}"-*-"${CARCH}".pkg.tar.zst
