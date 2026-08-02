<h1 align="center">Inference Gateway Desktop</h1>

<p align="center">
  A desktop AI client that works with any model provider - OpenAI, Anthropic, Google, local models, and everything in between.
</p>

<p align="center">
  <a href="https://github.com/inference-gateway/desktop/actions/workflows/ci.yml"><img src="https://github.com/inference-gateway/desktop/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/inference-gateway/desktop/actions/workflows/tasks.yml"><img src="https://github.com/inference-gateway/desktop/actions/workflows/tasks.yml/badge.svg" alt="OpenTask"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="License"></a>
</p>

Like Codex or Co-Work, but provider-agnostic. Bring your own API keys, pick your model, and work across providers from a single native window - no silos, no vendor lock-in.

Built with [Tauri](https://tauri.app) and powered by [Inference Gateway](https://docs.inference-gateway.com/).

## How it works

On first run, the app downloads the `infer` CLI binary and installs it to `~/.infer/bin/infer`. The CLI manages the gateway server and routes requests to whatever provider you configure - OpenAI, Anthropic, Google, local Ollama models, or any OpenAI-compatible endpoint.

The gateway binary lands at `~/.infer/bin/inference-gateway`, config lives under `~/.infer/`, and agent file tools are scoped to your home directory.

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
