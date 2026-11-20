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
npm run tauri build     # macOS: .app + .dmg in target/release/bundle/
```

### Linux
Install the WebKitGTK toolchain first, e.g. on Debian/Ubuntu:
```sh
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev build-essential
```
Then `npm run tauri build` produces `.deb`, `.rpm` and AppImage bundles.

## Limitations (v1)
- Chameleon includes (included schema without a targetNamespace) are not remapped
- WSDL 2.0, MTOM/attachments and WS-Addressing are not supported
- Credentials are stored in plain text in the project file
