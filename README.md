<h1 align="center">Inference Gateway Desktop</h1>

<p align="center">
  Cross-platform desktop client for <a href="https://inference-gateway.github.io">Inference Gateway</a>, built with <a href="https://tauri.app">Tauri</a>.
</p>

<p align="center">
  <a href="https://github.com/inference-gateway/desktop/actions/workflows/tasks.yml"><img src="https://github.com/inference-gateway/desktop/actions/workflows/tasks.yml/badge.svg" alt="CI"></a>
</p>

## Prerequisites

- **Rust toolchain** — install via [rustup](https://rustup.rs/)
- **Linux system dependencies** — on Debian/Ubuntu:

  ```bash
  sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
    libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```

  See the [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/) for other platforms.

## Running locally

```bash
# Install the Tauri CLI
cargo install tauri-cli --locked

# Start the dev server (opens a native window)
cargo tauri dev
```

## Building

```bash
cargo tauri build
```

The bundled app will be written to `src-tauri/target/release/bundle/`.
