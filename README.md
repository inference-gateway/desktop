<h1 align="center">Inference Gateway Desktop</h1>

<p align="center">
  A desktop AI client that works with any model provider - OpenAI, Anthropic, Google, local models, and everything in between.
</p>

<p align="center">
  <a href="https://github.com/inference-gateway/desktop/actions/workflows/ci.yml"><img src="https://github.com/inference-gateway/desktop/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/inference-gateway/desktop/actions/workflows/tasks.yml"><img src="https://github.com/inference-gateway/desktop/actions/workflows/tasks.yml/badge.svg" alt="OpenTask"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="License"></a>
  <img src="https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=white" alt="React">
  <img src="https://img.shields.io/badge/TypeScript-3178C6?logo=typescript&logoColor=white" alt="TypeScript">
  <img src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white" alt="Tauri">
</p>

Like Codex or Co-Work, but provider-agnostic. Bring your own API keys, pick your model, and work across providers from a single native window - no silos, no vendor lock-in.

Built with [Tauri](https://tauri.app) and a [React](https://react.dev) + [TypeScript](https://www.typescriptlang.org/) frontend (Bun + Vite, Tailwind v4 + shadcn/ui), powered by [Inference Gateway](https://docs.inference-gateway.com/).

## How it works

On first run, the app downloads the `infer` CLI binary and installs it to `~/.infer/bin/infer`. The CLI manages the gateway server and routes requests to whatever provider you configure - OpenAI, Anthropic, Google, local Ollama models, or any OpenAI-compatible endpoint.

The gateway binary lands at `~/.infer/bin/inference-gateway`, config lives under `~/.infer/`, and agent file tools are scoped to your home directory.

### Updating

The app updates itself. When a newer release is available the top bar shows an update button (the same one is in Settings under Updates); clicking it reinstalls the `infer` CLI and gateway binaries, then downloads the new app bundle, verifies its signature and relaunches. Checks run at startup and every 6 hours.

Releases are not signed with an Apple Developer or Windows code-signing certificate, so there is some **first-run** friction: macOS marks the downloaded `.dmg` as quarantined, so open the app once with right-click -> Open and confirm, and Windows SmartScreen asks for "More info" -> "Run anyway". Updates applied by the app itself are downloaded by the app rather than a browser, so they are not quarantined and do not repeat that prompt. macOS privacy permissions are separate: releases are signed with the project's own **self-signed** certificate (not from Apple, no developer account involved), which gives the app a stable code identity - permissions you grant survive updates. It does not remove the first-run Gatekeeper prompt; only a paid Apple Developer ID certificate would do that.

### Moving to a new machine

Settings > General > **Export / Import** moves the complete desktop state between machines: all Settings fields, sidebar projects, A2A agents, scheduled jobs, snippets, the skills registry URL and installed skills. Export writes one portable file (JSON, YAML or TOML) in a native save dialog, or pushes it to a private GitHub repo you name (created on demand; public repos are refused) - Import reads it back from either place and auto-detects the format. Credentials (database passwords, tokens, `auth.json` keys) are never exported, and machine-specific paths are stored `~/`-relative so they resolve against the new machine's home.

### Supported platforms

| Platform | Asset name |
| --- | --- |
| Linux amd64 | `infer-linux-amd64` |
| Linux arm64 | `infer-linux-arm64` |
| macOS amd64 | `infer-darwin-amd64` |
| macOS arm64 | `infer-darwin-arm64` |
| Windows amd64 | `infer-windows-amd64` |
| Windows arm64 | `infer-windows-arm64` |

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for prerequisites, local setup, and building.

### Testing macOS permission grants locally

macOS ties Accessibility and Screen Recording grants to the app's code-signing identity, so Computer Use permissions can only be verified from a signed `.app` bundle - dev builds (`task dev`) simulate the flow instead. The project signs with a **self-signed** certificate (created below with `openssl` - nothing from Apple, no developer account). To test the real thing:

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
