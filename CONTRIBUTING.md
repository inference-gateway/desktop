# Contributing

## Prerequisites

- **Flox** - the dev environment (Rust toolchain, `cargo`, `cargo-tauri`, `task`, `bun`, `infer`) is defined in the [flox manifest](.flox/env/manifest.toml). Install [flox](https://flox.dev), then run `flox activate` from the repo root.
- **Linux system dependencies** - on Debian/Ubuntu:

  ```bash
  sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
    libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```

  See the [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/) for other platforms.

## Running locally

```bash
flox activate
task install   # bun install (first run only)
task dev
```

`task dev` rebuilds the React frontend into `dist/` before launching Tauri. The frontend is not hot-reloaded - after editing `frontend/`, re-run `task web` (or relaunch).

## Building

```bash
cargo tauri build
```

The bundled app goes to `target/release/bundle/`. `task build` runs a plain `cargo build` (debug); use `cargo tauri build` for the release bundle.

`bundle.createUpdaterArtifacts` is on, so `cargo tauri build` needs the updater signing key in `TAURI_SIGNING_PRIVATE_KEY` (see below). `cargo tauri dev` and `cargo test` do not.

## Updater signing keys (maintainers)

Releases are signed with a minisign key pair that is independent of Apple/Windows code signing. It is generated once:

```bash
cargo tauri signer generate -w ~/.tauri/desktop.key
```

Then, in the repository settings:

- `TAURI_SIGNING_PRIVATE_KEY` (secret) - contents of `~/.tauri/desktop.key`.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (secret) - the password chosen above, if any.
- `TAURI_SIGNING_PUBLIC_KEY` (variable) - contents of `~/.tauri/desktop.key.pub`.

`release.yml` stamps the public key and the semantic-release version into `tauri.conf.json` before building, and fails early if either key is missing. Rotating the key means every already-installed app keeps trusting the old one, so a rotation needs a manual re-download.

## macOS self-signed code-signing certificate (maintainers)

macOS ties Accessibility and Screen Recording grants (TCC) to the app's code-signing identity. Ad-hoc signing changes identity every build, so grants break on each update. Releases are therefore signed with a long-lived **self-signed** certificate - created by the project itself, not issued by Apple, no Apple Developer account or fee involved. It gives the app a stable identity that keeps permission grants across updates. (A paid Apple Developer ID certificate with notarization would additionally remove the Gatekeeper first-open warning; the self-signed certificate does not.)

Generated once, in Keychain Access: Certificate Assistant > Create a Certificate, name `Inference Gateway Desktop Signing`, identity type Self-Signed Root, certificate type Code Signing, validity 3650 days. Export it as a `.p12` with a password, then in the repository settings:

- `APPLE_CERTIFICATE` (secret) - `base64 -i cert.p12` output. The name is Tauri's required env var; despite the name, it holds the self-signed certificate above, nothing from Apple.
- `APPLE_CERTIFICATE_PASSWORD` (secret) - the export password.

When these secrets are set, `release.yml` stamps `signingIdentity` into `tauri.conf.json`, adds the self-signed certificate to the runner's trust store (codesign refuses untrusted identities), and imports it into a keychain on the runner. The workflow does the import itself because Tauri's own `APPLE_CERTIFICATE` handling only recognizes Apple-issued certificates (it matches names like "Developer ID Application:"); with the env vars unset, the bundler signs through the configured `signingIdentity` instead, which works with any certificate in the keychain. Without the secrets, builds fall back to ad-hoc signing. Rotating the certificate invalidates every user's permission grants once.

## Testing macOS permission grants locally

macOS ties Accessibility and Screen Recording grants to the app's code-signing identity, so Computer Use permissions can only be verified from a signed `.app` bundle - dev builds without it report the permissions as unavailable, and the grant flow is simulated only when the app runs with `DESKTOP_MOCK=true` (for example `DESKTOP_MOCK=true INFER_GATEWAY_MOCK_SCENARIOS=e2e/scenarios.yaml task dev`). Note that `DESKTOP_MOCK` also switches the whole desktop into token-free mock mode: it skips the desktop-owned gateway, takes the model list from the `models:` block of the scenarios file named by `INFER_GATEWAY_MOCK_SCENARIOS` (without it the app reports an error and offers no models), and spawns `infer` with `INFER_GATEWAY_MOCK=true`, which serves its turns from that same scenarios file. The project signs with a **self-signed** certificate (created below with `openssl` - nothing from Apple, no developer account). To test the real thing:

1. Create the self-signed certificate once and trust it (approve the macOS prompt):

   ```bash
   openssl req -x509 -newkey rsa:2048 -keyout key.pem -out cert.pem -days 3650 -nodes \
     -subj "/CN=Inference Gateway Desktop Signing" \
     -addext "keyUsage=critical,digitalSignature" \
     -addext "extendedKeyUsage=critical,codeSigning" \
     -addext "basicConstraints=critical,CA:FALSE"
   openssl pkcs12 -export -legacy -out desktop-codesign.p12 -inkey key.pem -in cert.pem
   security import desktop-codesign.p12 -k ~/Library/Keychains/login.keychain-db -T /usr/bin/codesign
   security add-trusted-cert -p codeSign -r trustRoot -k ~/Library/Keychains/login.keychain-db cert.pem
   ```

2. Build the signed bundle and check its identity:

   ```bash
   export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/desktop.key)"
   cargo tauri build --config '{"bundle":{"macOS":{"signingIdentity":"Inference Gateway Desktop Signing"}}}'
   codesign -dr - "target/release/bundle/macos/Inference Gateway Desktop.app"
   ```

   The designated requirement must say `certificate leaf = H"..."`, not `cdhash` - that is what keeps grants stable across builds.

3. Clear any grants left by older ad-hoc builds, then launch:

   ```bash
   tccutil reset Accessibility com.inference-gateway.desktop
   tccutil reset ScreenCapture com.inference-gateway.desktop
   open "target/release/bundle/macos/Inference Gateway Desktop.app"
   ```

4. In Settings > General, click Grant on each permission and approve the OS prompts. Accessibility flips to Granted live; Screen Recording shows Granted after the app restarts.

5. Regression check: rebuild with the same self-signed certificate, relaunch, and confirm both permissions stay Granted with no new OS prompt. This is exactly what used to break with ad-hoc signing on every release.

## Project guide

See [AGENTS.md](AGENTS.md) for coding style, commit conventions, PR conventions, git hooks, and agent workflows.
