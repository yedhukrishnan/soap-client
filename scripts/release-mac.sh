#!/usr/bin/env bash
# Build, sign, notarize and staple a universal macOS release of SOAP Client.
#
# One-time setup (nothing here goes into the repo):
#   - A "Developer ID Application" identity in src-tauri/tauri.signing.json (git-ignored).
#   - An App Store Connect API key at ~/.appstoreconnect/AuthKey_<KEYID>.p8 (chmod 600).
#   - The key's Issuer ID in ~/.appstoreconnect/issuer-id, or in $APPLE_API_ISSUER.
#   - A Developer ID provisioning profile for dev.soapclient.app at
#     ~/.appstoreconnect/soap-client.provisionprofile (or $SOAP_PROVISIONING_PROFILE).
#     It grants the keychain entitlement that lets secrets use Touch ID.
#
# Usage:  npm run release:mac
# Output: release/<version>/SOAP-Client-<version>-macOS-universal.dmg and SHA256SUMS.txt
set -euo pipefail

cd "$(dirname "$0")/.."

say() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
die() { printf '\n\033[31mError:\033[0m %s\n' "$*" >&2; exit 1; }

# ---- credentials (from the environment or ~/.appstoreconnect; never from the repo) ----
KEY_DIR="${HOME}/.appstoreconnect"
if [[ -z "${APPLE_API_KEY_PATH:-}" ]]; then
  shopt -s nullglob
  keys=("${KEY_DIR}"/AuthKey_*.p8)
  shopt -u nullglob
  [[ ${#keys[@]} -eq 1 ]] || die "expected exactly one AuthKey_*.p8 in ${KEY_DIR} (or set APPLE_API_KEY_PATH)"
  APPLE_API_KEY_PATH="${keys[0]}"
fi
if [[ -z "${APPLE_API_KEY:-}" ]]; then
  APPLE_API_KEY="$(basename "${APPLE_API_KEY_PATH}" .p8)"
  APPLE_API_KEY="${APPLE_API_KEY#AuthKey_}"
fi
if [[ -z "${APPLE_API_ISSUER:-}" && -f "${KEY_DIR}/issuer-id" ]]; then
  APPLE_API_ISSUER="$(tr -d '[:space:]' < "${KEY_DIR}/issuer-id")"
fi
[[ -n "${APPLE_API_ISSUER:-}" ]] || die "no Issuer ID: put it in ${KEY_DIR}/issuer-id or set APPLE_API_ISSUER"
export APPLE_API_KEY APPLE_API_KEY_PATH APPLE_API_ISSUER   # read by `tauri build` to notarize the app

# ---- preconditions ----
[[ -f src-tauri/tauri.signing.json ]] || die "src-tauri/tauri.signing.json is missing (signing identity)"
grep -q '"signingIdentity"' src-tauri/tauri.signing.json || die "no signingIdentity in src-tauri/tauri.signing.json"
security find-identity -v -p codesigning | grep -q "Developer ID Application" \
  || die "no 'Developer ID Application' identity in the keychain"
for t in aarch64-apple-darwin x86_64-apple-darwin; do
  rustup target list --installed | grep -qx "$t" || die "missing Rust target $t (rustup target add $t)"
done
if [[ -n "$(git status --porcelain)" && "${ALLOW_DIRTY:-}" != "1" ]]; then
  die "working tree has uncommitted changes (commit them, or set ALLOW_DIRTY=1)"
fi

# ---- provisioning profile -> entitlements (Touch ID keychain) ----
BUNDLE_ID="$(node -p "require('./src-tauri/tauri.conf.json').identifier")"
PROFILE="${SOAP_PROVISIONING_PROFILE:-${KEY_DIR}/soap-client.provisionprofile}"
[[ -f "$PROFILE" ]] || die "no provisioning profile at ${PROFILE}"
GEN="target/release-config"
mkdir -p "$GEN"
security cms -D -i "$PROFILE" > "${GEN}/profile.plist" 2>/dev/null || die "can't read provisioning profile ${PROFILE}"
pb() { /usr/libexec/PlistBuddy -c "Print :$1" "${GEN}/profile.plist" 2>/dev/null; }
TEAM_ID="$(pb 'TeamIdentifier:0')"
APP_ID="$(pb 'Entitlements:com.apple.application-identifier')"
[[ "$APP_ID" == "${TEAM_ID}.${BUNDLE_ID}" ]] || die "profile is for '${APP_ID}', expected '<team>.${BUNDLE_ID}'"
[[ "$(pb 'ProvisionsAllDevices')" == "true" ]] || die "profile is not a Developer ID profile"
EXPIRES="$(plutil -extract ExpirationDate raw -o - "${GEN}/profile.plist")"   # ISO 8601, UTC
[[ "$EXPIRES" > "$(date -u +%Y-%m-%dT%H:%M:%SZ)" ]] || die "provisioning profile expired (${EXPIRES})"
SIGN_ID="$(node -p "require('./src-tauri/tauri.signing.json').bundle.macOS.signingIdentity")"
security find-identity -v -p codesigning | grep "$SIGN_ID" | grep -q "(${TEAM_ID})" \
  || die "signing identity isn't from the profile's team"

cat > "${GEN}/entitlements.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.application-identifier</key>
  <string>${APP_ID}</string>
  <key>com.apple.developer.team-identifier</key>
  <string>${TEAM_ID}</string>
  <key>keychain-access-groups</key>
  <array>
    <string>${APP_ID}</string>
  </array>
</dict>
</plist>
PLIST
node -e '
  const [signing, ents, profile, out] = process.argv.slice(1);
  const c = JSON.parse(require("fs").readFileSync(signing, "utf8"));
  c.bundle.macOS.entitlements = ents;
  c.bundle.macOS.files = { "embedded.provisionprofile": profile };
  require("fs").writeFileSync(out, JSON.stringify(c, null, 2));
' src-tauri/tauri.signing.json "$(pwd)/${GEN}/entitlements.plist" "$PROFILE" "${GEN}/tauri.release.json"

VERSION="$(node -p "require('./package.json').version")"
for f in src-tauri/tauri.conf.json src-tauri/Cargo.toml; do
  grep -Eq "\"?version\"? *[:=] *\"${VERSION}\"" "$f" || die "version in $f doesn't match package.json (${VERSION})"
done

BUNDLE="target/universal-apple-darwin/release/bundle"
OUT="release/${VERSION}"
APP="${BUNDLE}/macos/SOAP Client.app"
DMG_NAME="SOAP-Client-${VERSION}-macOS-universal.dmg"

# Leftover temporary images from an interrupted DMG step make the next one fail.
detach_leftovers() {
  hdiutil info | awk '/image-path.*bundle\/(macos|dmg)\/rw\./{f=1} f&&/^\/dev\/disk[0-9]+[ \t]/{print $1; f=0}' |
    while read -r dev; do hdiutil detach "$dev" -force -quiet || true; done
  rm -f "${BUNDLE}"/macos/rw.*.dmg
}

# ---- build, sign and notarize the app ----
say "Building SOAP Client ${VERSION} (universal), signing and notarizing the app"
detach_leftovers
build() { npx tauri build --config "${GEN}/tauri.release.json" --target universal-apple-darwin; }
if ! build; then
  say "Build failed; cleaning up and retrying once (the DMG step is occasionally flaky)"
  detach_leftovers
  build
fi

shopt -s nullglob
dmgs=("${BUNDLE}"/dmg/*.dmg)
shopt -u nullglob
[[ ${#dmgs[@]} -eq 1 ]] || die "expected one .dmg in ${BUNDLE}/dmg"
DMG="${dmgs[0]}"

# ---- notarize and staple the disk image itself ----
say "Notarizing the disk image (this usually takes a few minutes)"
xcrun notarytool submit "$DMG" \
  --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER" --wait
xcrun stapler staple "$DMG"

# ---- verify ----
say "Verifying"
codesign --verify --deep --strict "$APP"
[[ -f "${APP}/Contents/embedded.provisionprofile" ]] || die "provisioning profile not embedded"
codesign -d --entitlements - --xml "$APP" 2>/dev/null | grep -q "keychain-access-groups" \
  || die "keychain entitlement missing from the signature"
xcrun stapler validate "$APP"
xcrun stapler validate "$DMG"
spctl -a -t exec -vv "$APP" 2>&1 | tee /dev/stderr | grep -q "source=Notarized Developer ID" \
  || die "Gatekeeper does not accept the app as notarized"
spctl -a -t open --context context:primary-signature -vv "$DMG" 2>&1 | tee /dev/stderr \
  | grep -q "source=Notarized Developer ID" || die "Gatekeeper does not accept the disk image as notarized"
lipo -archs "${APP}/Contents/MacOS/soap-client"

# ---- package ----
mkdir -p "$OUT"
cp "$DMG" "${OUT}/${DMG_NAME}"
(cd "$OUT" && shasum -a 256 "$DMG_NAME" > SHA256SUMS.txt)

say "Done: ${OUT}/${DMG_NAME}"
cat "${OUT}/SHA256SUMS.txt"
cat <<EOF

To publish (see docs/RELEASING.md): push the tag v${VERSION} so GitHub builds the Linux
packages into a draft release, then add this disk image and publish:
  gh release upload v${VERSION} "${OUT}/${DMG_NAME}" "${OUT}/SHA256SUMS.txt"
  gh release edit v${VERSION} --draft=false --notes-file <release-notes.md>
EOF
