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

Releases are not signed with an Apple Developer or Windows code-signing certificate, so there is some **first-run** friction: macOS marks the downloaded `.dmg` as quarantined, so open the app once with right-click -> Open and confirm, and Windows SmartScreen asks for "More info" -> "Run anyway". Updates applied by the app itself are downloaded by the app rather than a browser, so they are not quarantined and do not repeat that prompt. macOS privacy permissions are separate: without a Developer ID certificate the app's code identity changes with every build, so a permission you grant may need re-approving after an update.

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
