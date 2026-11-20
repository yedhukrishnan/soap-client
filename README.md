# SOAP Client

A lightweight, SoapUI-style desktop SOAP client for macOS and Linux, built with Rust + Tauri 2 + Svelte 5.

## Features
- Import a WSDL 1.1 from a URL or file; `wsdl:import`, `xsd:import`/`include`/`redefine` are followed recursively (relative to the referencing document) and stored in the project; browse bindings → operations
- SoapUI-style sample requests generated from the XSD (optional/repeat/choice comments, enums, attributes, extensions, recursion guard)
- SOAP 1.1 and 1.2, document/literal and rpc/literal, `soap:header` parts
- Tabbed requests with a Monaco XML editor; pretty / raw / headers / sent-request views of the response
- Auth: HTTP Basic, WS-Security UsernameToken (PasswordText or PasswordDigest, optional Timestamp), custom HTTP headers
- Assertions: HTTP status, contains / not contains, XPath 1.0 match, no SOAP Fault, max response time
- Projects saved as `*.soapproj.json` (WSDL text is embedded, so projects work offline)
- **Autosave**: the whole working session (project, open tabs, active environment) is saved continuously
  to the app data folder and restored on the next launch — closing the app never loses work.
  `⌘S` still writes the project to its own file for sharing.
- **Variables & environments**: use `{{name}}` in the body, endpoint, SOAPAction, headers or auth fields.
  Project variables are defaults; environments (e.g. one per supplier or stage) override them and are
  switched from the sidebar. Endpoints can be per environment too: the `{{ }}` button next to a URL moves it
  into a variable (optionally for every request using that URL), the resolved URL is shown under the
  endpoint, and new requests for the same binding reuse the templated endpoint. Values are XML-escaped when inserted into the body; undefined variables
  are highlighted in the editor and block sending.
- **Secrets**: secret variables are stored in the OS credential store (macOS Keychain, Linux Secret
  Service) — never in the project file or the autosaved session — and are masked as `••••••` in the
  sent-request view. The app only ever reads its own entries (service `dev.soapclient.app`; the service
  name is fixed in code), never other Keychain items. A one-time note explains the macOS Keychain
  prompt before it first appears. Literal passwords typed into a request body (`<password>`, `<token>`, …) or the
  Auth tab are detected, with a one-click *Move to secret variable*.

Requests can be duplicated (⧉ in the sidebar, or `⌘/Ctrl+D`); you're asked for the new name.

Shortcuts: `⌘/Ctrl+Enter` send · `⌘/Ctrl+D` duplicate · `⌘/Ctrl+S` save · `⇧⌘/Ctrl+Shift+S` save as · `⌘/Ctrl+O` open.

## Layout
```
crates/soap-core   Pure Rust library: WSDL/XSD parsing, sample generation, envelope + WS-Security,
                   HTTP (reqwest/rustls), assertions, project files. No UI dependencies.
src-tauri          Thin Tauri command layer (src/commands.rs)
ui                 Svelte 5 + TypeScript frontend (Vite)
```

## Develop
Prerequisites: Rust (stable), Node 20+.

```sh
npm install
npm run tauri dev       # run the app with hot reload
cargo test              # core tests
cargo test -- --ignored # also run the live test against dneonline.com
npm run check           # svelte-check / TypeScript
```

## Build
```sh
npm run tauri build     # macOS: .app + .dmg in target/release/bundle/ (unsigned)
npm run build:mac       # same, code-signed (see below)
```

### Code signing (macOS)
Signing gives the app a stable identity, so a Keychain "Always Allow" survives rebuilds.
`build:mac` reads the identity from `src-tauri/tauri.signing.json`, which is git-ignored:
```json
{ "bundle": { "macOS": { "signingIdentity": "<identity name or SHA-1>" } } }
```
List usable identities with `security find-identity -v -p codesigning` (an "Apple Development"
certificate works, as does a self-signed code-signing certificate from Keychain Access).

### Linux
Install the WebKitGTK toolchain first, e.g. on Debian/Ubuntu:
```sh
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev build-essential
```
Then `npm run tauri build` produces `.deb`, `.rpm` and AppImage bundles.

## Limitations (v1)
- Chameleon includes (included schema without a targetNamespace) are not remapped
- WSDL 2.0, MTOM/attachments and WS-Addressing are not supported
- Without a Secret Service (some minimal Linux setups) secrets are kept in memory for the current run only
- Unsigned builds (`tauri dev`, `tauri build`): macOS may ask again for Keychain access after each rebuild

## Author
Yedhu Krishnan · [yedhu.me](https://yedhu.me) · dev@yedhu.me
