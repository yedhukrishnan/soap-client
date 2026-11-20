#!/usr/bin/env bash
# Runs inside the packaging/linux container: builds the .deb from the source tree it is in
# and copies it to OUT_DIR as soap-client_<version>_<arch>.deb.
#
# Usage (from the host): git archive HEAD | docker run --rm -i -v "$PWD/out:/out" IMAGE \
#   bash -c 'mkdir /build && tar -x -C /build && /build/packaging/linux/build-deb.sh /out'
set -euo pipefail

OUT_DIR="${1:?usage: build-deb.sh <out-dir>}"
cd "$(dirname "$0")/../.."

VERSION="$(node -p "require('./package.json').version")"
ARCH="$(dpkg --print-architecture)"
NAME="soap-client_${VERSION}_${ARCH}.deb"

npm ci --no-audit --no-fund
npx tauri build --bundles deb

shopt -s nullglob
debs=(target/release/bundle/deb/*.deb)
[[ ${#debs[@]} -eq 1 ]] || { echo "expected one .deb, found ${#debs[@]}" >&2; exit 1; }

mkdir -p "$OUT_DIR"
cp "${debs[0]}" "${OUT_DIR}/${NAME}"

echo "== ${NAME}"
dpkg-deb --info "${OUT_DIR}/${NAME}" | sed -n '/Package:/,$p'
dpkg-deb --contents "${OUT_DIR}/${NAME}"
echo "Highest glibc symbol version required:"
objdump -T target/release/soap-client | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1

# Files written to a bind mount would otherwise be owned by root.
if [[ -n "${HOST_UID:-}" ]]; then chown "${HOST_UID}:${HOST_GID:-$HOST_UID}" "$OUT_DIR" "${OUT_DIR}/${NAME}"; fi
