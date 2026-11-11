<h1 align="center">Inference Gateway Desktop</h1>

<p align="center">
  A native desktop app for AI agents that work on your computer - with any model provider and your own API keys.
</p>

<p align="center">
  <a href="https://github.com/inference-gateway/desktop/actions/workflows/ci.yml"><img src="https://github.com/inference-gateway/desktop/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/inference-gateway/desktop/actions/workflows/tasks.yml"><img src="https://github.com/inference-gateway/desktop/actions/workflows/tasks.yml/badge.svg" alt="OpenTask"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="License"></a>
  <img src="https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=white" alt="React">
  <img src="https://img.shields.io/badge/TypeScript-3178C6?logo=typescript&logoColor=white" alt="TypeScript">
  <img src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white" alt="Tauri">
</p>

Like Codex or Co-Work, but provider-agnostic: OpenAI, Anthropic, Google, local Ollama models or any OpenAI-compatible endpoint, all from one window. No silos, no vendor lock-in.

## What you can do

- **Chat and code in your projects** - the agent runs in the project directory you pick, with file tools sandboxed to it.
- **Give agents their own computer** - run an agent on a Kubernetes workspace, watch its screen under **Computers**, and take over the mouse and keyboard when it gets stuck. Each workspace has its own **Computer use**, **Browser use** and **Recording** switches in Settings > Workspaces, on by default - the ones in Settings > General cover this computer only. A workspace can also mount **volumes** - existing claims of its namespace, read-only where the agent should copy before it edits - so a heavy app or raw footage is not downloaded to every machine. On the local cluster, Settings > Compute shares **folders of this computer** with its workspaces - mounted straight from here with no copy, read-only for every workspace or for one.
- **Drive your browser** - the [opentask](https://github.com/inference-gateway/opentask) extension lets the agent navigate, click, type and read tabs in your everyday browser.
- **Write your own agents** - Markdown files with a name, instructions, model and face. Agents can hand work to each other.
- **Make content** - talking-avatar clips, text to speech and text to video on a timeline in Content projects.
- **Stay portable** - the app updates itself, and Export / Import moves your whole setup to a new machine.

## Install

Download the asset for your platform from the [latest release](https://github.com/inference-gateway/desktop/releases/latest):

| Platform | Download |
| --- | --- |
| macOS (Apple Silicon and Intel) | `.dmg` |
| Linux x86_64 | `.AppImage`, `.deb` or `.rpm` |
| Windows x64 | `-setup.exe` or `.msi` |

Releases are not signed by Apple or Microsoft, so the first launch needs one extra click: on macOS right-click > Open and confirm, on Windows choose "More info" > "Run anyway". On first run the app downloads the `infer` CLI and the gateway into `~/.infer/` (on Windows you install the gateway yourself, see [How it works](docs/how-it-works.md)). Then add your provider keys under Settings > API Keys and start chatting.

## Learn more

- [How it works](docs/how-it-works.md) - the gateway and CLI, working directory and sandbox, updates, supported platforms.
- [Agents and computers](docs/agents.md) - write your own agents and give them a machine of their own.
- [Browser use](docs/browser-use.md) - connect the opentask extension.
- [Avatars](docs/avatars.md) - lip-synced talking clips of yourself in Content projects.
- [Export / Import](docs/export-import.md) - move your setup to a new machine.
- [examples/computer-use](examples/computer-use) - three agents on Ubuntu desktops in a local Kubernetes cluster.
- [examples/filmcraft](examples/filmcraft) - an editor agent with FilmCraft on every computer, from a folder shared with the local cluster.

## Development

Built with [Tauri](https://tauri.app) and a [React](https://react.dev) + [TypeScript](https://www.typescriptlang.org/) frontend (Bun + Vite, Tailwind v4 + shadcn/ui), powered by [Inference Gateway](https://docs.inference-gateway.com/). See [CONTRIBUTING.md](CONTRIBUTING.md) for prerequisites, local setup and building.
