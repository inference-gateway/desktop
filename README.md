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

On first run, the app downloads the `infer` CLI binary and installs it to `~/.infer/bin/infer`. The desktop itself downloads, runs and restarts the gateway server, which routes requests to whatever provider you configure - OpenAI, Anthropic, Google, local Ollama models, or any OpenAI-compatible endpoint. The `infer` CLI detects the running gateway and does not start its own.

The gateway binary lands at `~/.infer/bin/inference-gateway` (`inference-gateway.exe` on Windows) and config lives under `~/.infer/`. On Windows the desktop does not download the gateway automatically, so either that binary must already be installed at the path above, or a gateway must already be running at the configured gateway URL - the app reuses one it finds serving that URL. The agent runs in the selected project's directory. A chat with no project runs in the directory the app itself was launched from - one level up when `cargo tauri dev` starts it from `backend/` - and falls back to `~/.infer/workspace` only when that directory is `/`, as it is for a Finder-launched `.app`, or cannot be determined. So a chat started from a terminal or a Linux/Windows launcher works in that launch directory. Its file tools are sandboxed to that working directory, `/tmp`, and every project directory under the configured projects root. The app also installs a bundled `desktop-projects` skill under `~/.infer/skills/`, so asking the agent to organise projects or chats (for example `/desktop-projects organise my projects under ~/Repositories`) edits `~/.infer/projects.yaml` and the projects root instead of the app.

### Updating

The app updates itself. When a newer release is available the top bar shows an update button (the same one is in Settings under Updates); clicking it reinstalls the `infer` CLI and gateway binaries, then downloads the new app bundle, verifies its signature and relaunches. Checks run at startup and every 6 hours. On Windows the gateway reinstall step does not run, for the same reason as above: the gateway is not downloaded automatically there, so the binary at `~/.infer/bin/inference-gateway.exe` is left as is unless you update it yourself or point the gateway URL at a gateway you run, and the gateway already serving that URL keeps running across the update.

Releases are not signed with an Apple Developer or Windows code-signing certificate, so there is some **first-run** friction: macOS marks the downloaded `.dmg` as quarantined, so open the app once with right-click -> Open and confirm, and Windows SmartScreen asks for "More info" -> "Run anyway". Updates applied by the app itself are downloaded by the app rather than a browser, so they are not quarantined and do not repeat that prompt. macOS privacy permissions are separate: releases are signed with the project's own **self-signed** certificate (not from Apple, no developer account involved), which gives the app a stable code identity - permissions you grant survive updates. It does not remove the first-run Gatekeeper prompt; only a paid Apple Developer ID certificate would do that.

### Browser use (opentask extension)

Settings > General > **Browser Use** lets the agent drive your everyday browser through the [opentask](https://github.com/inference-gateway/opentask) extension - navigate, click, type, read, screenshot and list tabs - which is usually cheaper and more reliable than Computer Use. Enabling it writes `~/.infer/browser_use.yaml` (`enabled: true`, `backend: extension`, and a generated `extension.token` if none is set) and restarts the `infer daemon` the app runs its chats through. The daemon binds `127.0.0.1:52789` (`binding.port` in `~/.infer/daemon.yaml`, or `extension.port`) for the extension, the desktop and any standalone `infer` session alike; paste the port and token shown in Settings into the extension options and the globe icon in the status bar (next to the auto-approve bolt) turns green once it connects, as the daemon reports the extension's state. Every chat is a thread of the daemon, so the agent's `browser_*` tools reach the extension through it, and the extension's side panel shows the same threads.

### Avatars (talking clips)

Settings > **Avatars** keeps named portraits of you that the agent turns into lip-synced talking clips in Content projects. Import a front-facing photo or take one with the camera. The app runs `infer avatars create <name> --from <photo>`, which keeps the photo and generates two three-quarter views of it through the gateway's image edit API. Then turn on **Text to video** in Settings > General (off by default) and ask for a talking avatar, presenter or talking head in a Content project. The bundled `video-editing` skill does the rest:

- It writes the script and voice as spoken clips with `TextToSpeech`.
- It renders each clip with the CLI's `TextToVideo` tool, passing `avatar` and `audio`.
- It places each rendered clip on the video track over its spoken clip, marked with `avatar`. On the timeline the clip is ordinary footage labelled with the avatar's name, and Export needs nothing extra.

Each avatar is a folder of portraits of one person, and the CLI owns the library (`infer avatars list` / `delete`):

```text
~/.infer/avatars/
  presenter/
    01-front.jpeg              # the first image in sort order is the one that talks
    02-three-quarter-left.png
    03-three-quarter-right.png
```

Use only your own likeness or one you have rights to. The photo is sent to the image edit provider (`tools.image_edit.model`, OpenAI by default) when the extra views are generated. The portrait and the voice clip are sent to the video provider on every render. Nothing is stored as an avatar at the provider. Renders go through the gateway's Videos API with ElevenLabs `creatify-aurora`, so add an ElevenLabs key under Settings > API Keys. Saving the toggle restarts the gateway with `VIDEOS_ENABLED=true`.

### Moving to a new machine

Settings > General > **Export / Import** moves the complete desktop state between machines: all Settings fields, sidebar projects, A2A agents, scheduled jobs, snippets, the skills registry URL and installed skills. Export writes one portable file (JSON, YAML or TOML) in a native save dialog, or pushes it to a private GitHub repo you name (created on demand; public repos are refused) - Import reads it back from either place and auto-detects the format. Credentials (database passwords, tokens, `auth.yaml` keys) are never exported, and machine-specific paths are stored `~/`-relative so they resolve against the new machine's home. Projects that are GitHub checkouts travel as their `owner/name` rather than a full copy: Import re-clones them under the projects root and re-reads their `AGENTS.md`, keeping the file small (edited project instructions are still carried in full).

### Supported platforms

| Platform | Asset name |
| --- | --- |
| Linux amd64 | `infer-linux-amd64` |
| Linux arm64 | `infer-linux-arm64` |
| macOS amd64 | `infer-darwin-amd64` |
| macOS arm64 | `infer-darwin-arm64` |
| Windows amd64 | `infer-windows-amd64` |
| Windows arm64 | `infer-windows-arm64` |

Those are the `infer` CLI assets. The app itself is released for fewer: a universal macOS `.dmg` (Apple Silicon and Intel), Linux x86_64 (AppImage, deb and rpm) and Windows x64 (setup.exe and msi). There is no Linux arm64 or Windows arm64 app bundle, and the updater serves exactly those four targets (`darwin-aarch64`, `darwin-x86_64`, `linux-x86_64`, `windows-x86_64`). Its prebuilt content and voice tools (ffmpeg, whisper-cli) come from the [binaries release](https://github.com/inference-gateway/binaries) and are managed by the CLI in `~/.infer/bin/tools`: the app checks them with `infer binaries status` and runs `infer binaries install` for any that are missing or stale (sha256 no longer matching the release), so an outdated copy is upgraded and a current one is kept. They are published for Apple Silicon macOS, Linux amd64/arm64 and Windows amd64 only - the Intel Mac (macOS x86_64) assets were dropped in binaries v0.5.0 and the app skips the install there instead of failing. On an Intel Mac, install the tools yourself and the app picks them up from PATH: voice input needs a whisper.cpp CLI (`brew install whisper-cpp`, or point `WHISPER_BIN` at the binary), and content projects need a full ffmpeg with H.264 encoding (`brew install ffmpeg`) for keyframes and export.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for prerequisites, local setup, and building.

### Testing macOS permission grants locally

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
