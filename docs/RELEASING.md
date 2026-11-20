# Releasing

A release has macOS and Linux packages. Linux packages are built by GitHub Actions when a
version tag is pushed; the macOS build is signed and notarized locally.

## macOS

`npm run release:mac` builds a universal app (Apple Silicon and Intel), signs it with a
*Developer ID Application* certificate, notarizes and staples both the app and the disk image,
checks them with Gatekeeper, and writes the `.dmg` and `SHA256SUMS.txt` to `release/<version>/`.

### One-time setup

Nothing below goes into the repository.

- **Signing identity:** the *Developer ID Application* certificate, named in `src-tauri/tauri.signing.json`:
  ```json
  { "bundle": { "macOS": { "signingIdentity": "<identity name or SHA-1>" } } }
  ```
  List identities with `security find-identity -v -p codesigning`.
- **Provisioning profile:** a Developer ID profile for `dev.soapclient.app` at
  `~/.appstoreconnect/soap-client.provisionprofile`. It grants the keychain entitlement used for Touch ID.
- **Notarization key:** an App Store Connect API key (role *Developer*) at
  `~/.appstoreconnect/AuthKey_<KEYID>.p8`, with its Issuer ID in `~/.appstoreconnect/issuer-id`.
  Alternatively set `APPLE_API_KEY_PATH`, `APPLE_API_KEY` and `APPLE_API_ISSUER`.
- **Intel target:** `rustup target add x86_64-apple-darwin`.

## Linux

`.github/workflows/release-linux.yml` runs on every pushed `v*` tag. It builds the `.deb`
(x86_64 and arm64) in an Ubuntu 22.04 container, so it runs on Ubuntu 22.04+ and Debian 12+;
repackages the x86_64 `.deb` for Arch Linux (`soap-client-bin`, see `packaging/arch/PKGBUILD`);
installs each package on a clean system to check its libraries resolve; and attaches the
packages and `SHA256SUMS-linux.txt` to the tag's release, creating it as a draft if needed.
No secrets are needed. Run it by hand from the Actions tab to get the packages as artifacts
without releasing.

To build locally for this machine's architecture, run `npm run release:linux`. It needs Docker
(your user in the `docker` group, or `DOCKER="sudo docker"`), and builds the Arch package too when
run on Arch Linux (this is the only way to get an aarch64 Arch package). Output goes to `release/<version>/`.

## Publishing a release

1. Set the same version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, and commit.
2. Tag and push; the Linux packages build on GitHub and land in a draft release:

   ```sh
   git push origin main
   git tag v<version> && git push origin v<version>
   ```
3. Run `npm run release:mac` and add the disk image to the draft:

   ```sh
   gh release upload v<version> release/<version>/*.dmg release/<version>/SHA256SUMS.txt
   ```
4. Once the Linux workflow has finished, publish:

   ```sh
   gh release edit v<version> --draft=false --notes-file <release-notes.md>
   ```
5. Tell installed apps about the release: update `latest.json` on the `gh-pages` branch
   (served at `https://yedhu.me/soap-client/latest.json`). Add the version to `securityReleases`
   if the release fixes a security issue; apps then show a more prominent notice that can't be skipped.

   ```json
   { "version": "<version>", "securityReleases": ["0.2.1", "0.2.2"] }
   ```

   Only publish it after the release, because the notice links to the latest release.

## Other macOS builds

- `npm run tauri build`: unsigned build. Secrets use the login keychain, and macOS may ask for
  Keychain access again after every rebuild.
- `npm run build:mac`: signed with the identity in `tauri.signing.json`, without notarization.
