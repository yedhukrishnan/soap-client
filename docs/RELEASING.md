# Releasing (macOS)

`npm run release:mac` builds a universal app (Apple Silicon and Intel), signs it with a
*Developer ID Application* certificate, notarizes and staples both the app and the disk image,
checks them with Gatekeeper, and writes the `.dmg` and `SHA256SUMS.txt` to `release/<version>/`.

## One-time setup

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

## Publishing a release

1. Set the same version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, and commit.
2. Run `npm run release:mac`.
3. Tag and publish:

   ```sh
   git push origin main
   git tag v<version> && git push origin v<version>
   gh release create v<version> release/<version>/*.dmg release/<version>/SHA256SUMS.txt \
     --title "SOAP Client <version>" --notes-file <release-notes.md>
   ```

## Other builds

- `npm run tauri build`: unsigned build. Secrets use the login keychain, and macOS may ask for
  Keychain access again after every rebuild.
- `npm run build:mac`: signed with the identity in `tauri.signing.json`, without notarization.
