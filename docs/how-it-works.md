# How it works

On first run, the app downloads the `infer` CLI binary and installs it to `~/.infer/bin/infer`. The desktop itself downloads, runs and restarts the gateway server, which routes requests to whatever provider you configure - OpenAI, Anthropic, Google, local Ollama models, or any OpenAI-compatible endpoint. The `infer` CLI detects the running gateway and does not start its own.

The gateway binary lands at `~/.infer/bin/inference-gateway` (`inference-gateway.exe` on Windows) and config lives under `~/.infer/`. On Windows the desktop does not download the gateway automatically, so either that binary must already be installed at the path above, or a gateway must already be running at the configured gateway URL - the app reuses one it finds serving that URL.

## Working directory and sandbox

The agent runs in the selected project's directory. A chat with no project runs in the directory the app itself was launched from - one level up when `cargo tauri dev` starts it from `backend/` - and falls back to `~/.infer/workspace` only when that directory is `/`, as it is for a Finder-launched `.app`, or cannot be determined. So a chat started from a terminal or a Linux/Windows launcher works in that launch directory. Its file tools are sandboxed to that working directory, `/tmp`, and every project directory under the configured projects root.

The app also installs a bundled `desktop-projects` skill under `~/.infer/skills/`, so asking the agent to organise projects or chats (for example `/desktop-projects organise my projects under ~/Repositories`) edits `~/.infer/projects.yaml` and the projects root instead of the app.

## Updating

The app updates itself. When a newer release is available the top bar shows an update button (the same one is in Settings under Updates). Clicking it reinstalls the `infer` CLI and gateway binaries, then downloads the new app bundle, verifies its signature and relaunches. Checks run at startup and every 6 hours.

On Windows the gateway reinstall step does not run, for the same reason as above: the gateway is not downloaded automatically there, so the binary at `~/.infer/bin/inference-gateway.exe` is left as is unless you update it yourself or point the gateway URL at a gateway you run, and the gateway already serving that URL keeps running across the update.

## First-run prompts and code signing

Releases are not signed with an Apple Developer or Windows code-signing certificate, so there is some **first-run** friction: macOS marks the downloaded `.dmg` as quarantined, so open the app once with right-click -> Open and confirm, and Windows SmartScreen asks for "More info" -> "Run anyway". Updates applied by the app itself are downloaded by the app rather than a browser, so they are not quarantined and do not repeat that prompt.

macOS privacy permissions are separate: releases are signed with the project's own **self-signed** certificate (not from Apple, no developer account involved), which gives the app a stable code identity - permissions you grant survive updates. It does not remove the first-run Gatekeeper prompt. Only a paid Apple Developer ID certificate would do that.

## Supported platforms

The app is released as a universal macOS `.dmg` (Apple Silicon and Intel), Linux x86_64 (AppImage, deb and rpm) and Windows x64 (setup.exe and msi). There is no Linux arm64 or Windows arm64 app bundle, and the updater serves exactly those four targets (`darwin-aarch64`, `darwin-x86_64`, `linux-x86_64`, `windows-x86_64`).

The `infer` CLI the app installs is published for more targets, each a tarball holding one `infer` executable (`infer.exe` on Windows):

| Platform | Asset name |
| --- | --- |
| Linux amd64 | `infer-linux-amd64.tar.gz` |
| Linux arm64 | `infer-linux-arm64.tar.gz` |
| macOS amd64 | `infer-darwin-amd64.tar.gz` |
| macOS arm64 | `infer-darwin-arm64.tar.gz` |
| Windows amd64 | `infer-windows-amd64.tar.gz` |
| Windows arm64 | `infer-windows-arm64.tar.gz` |

### Content and voice tools

The prebuilt content and voice tools (ffmpeg, whisper-cli) come from the [binaries release](https://github.com/inference-gateway/binaries) and are managed by the CLI in `~/.infer/bin/tools`. The app checks them with `infer binaries status` and runs `infer binaries install` for any that are missing or stale (sha256 no longer matching the release), so an outdated copy is upgraded and a current one is kept.

They are published for Apple Silicon macOS, Linux amd64/arm64 and Windows amd64 only - the Intel Mac (macOS x86_64) assets were dropped in binaries v0.5.0 and the app skips the install there instead of failing. On an Intel Mac, install the tools yourself and the app picks them up from PATH: voice input needs a whisper.cpp CLI (`brew install whisper-cpp`, or point `WHISPER_BIN` at the binary), and content projects need a full ffmpeg with H.264 encoding (`brew install ffmpeg`) for keyframes and export.
