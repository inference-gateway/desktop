<h1 align="center">Inference Gateway Desktop</h1>

<p align="center">
  Cross-platform desktop client for <a href="https://inference-gateway.github.io">Inference Gateway</a>, built with <a href="https://tauri.app">Tauri</a>.
</p>

<p align="center">
  <a href="https://github.com/inference-gateway/desktop/actions/workflows/ci.yml"><img src="https://github.com/inference-gateway/desktop/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/inference-gateway/desktop/actions/workflows/tasks.yml"><img src="https://github.com/inference-gateway/desktop/actions/workflows/tasks.yml/badge.svg" alt="OpenTask"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="License"></a>
</p>

## Prerequisites

- **Flox** — the dev environment (Rust 1.95 toolchain, `task`, `infer`) is defined in the repo's [flox manifest](.flox/env/manifest.toml). Install [flox](https://flox.dev), then run `flox activate` from the repo root.
- **Linux system dependencies** — on Debian/Ubuntu:

  ```bash
  sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
    libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```

  See the [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/) for other platforms.

## Running locally

```bash
# Enter the dev environment (Rust 1.95 toolchain, task, infer)
flox activate

# Start the dev server (opens a native window)
task dev
```

The flox manifest doesn't include `cargo` or the Tauri CLI yet — add them once from the repo root with `flox install cargo cargo-tauri`.

## Building

```bash
cargo tauri build
```

The bundled app will be written to `src-tauri/target/release/bundle/`. `task build` runs a plain `cargo build` (debug); use `cargo tauri build` for the release bundle.

## How it works

### CLI binary management

On first run, the app downloads the `infer` CLI binary from the
[inference-gateway/cli](https://github.com/inference-gateway/cli) releases
and installs it to `~/.infer/bin/infer`. The download is verified against
the release `checksums.txt` before the file is made executable; a partial
or tampered download is discarded.

The CLI is always spawned with its working directory set to `$HOME`. This
means:

- The gateway binary (`inference-gateway`) is placed at
  `~/.infer/bin/inference-gateway` by the CLI's own `gateway_manager.go`,
  which resolves `filepath.Join(".infer", "bin")` relative to its working
  directory — no download code needed on our side.
- The config layer lives under `~/.infer/`.
- Agent file tools are scoped to the home directory.

A workspace picker that lets the user point a session at a project
directory is a good follow-up.

### Supported platforms

| Platform | Asset name |
| --- | --- |
| Linux amd64 | `infer-linux-amd64` |
| Linux arm64 | `infer-linux-arm64` |
| macOS amd64 | `infer-darwin-amd64` |
| macOS arm64 | `infer-darwin-arm64` |
| Windows amd64 | `infer-windows-amd64` |
| Windows arm64 | `infer-windows-arm64` |
