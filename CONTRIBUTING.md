# Contributing

## Prerequisites

- **Flox** — the dev environment (Rust 1.95 toolchain, `task`, `infer`) is defined in the [flox manifest](.flox/env/manifest.toml). Install [flox](https://flox.dev), then run `flox activate` from the repo root.
- **Linux system dependencies** — on Debian/Ubuntu:

  ```bash
  sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
    libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```

  See the [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/) for other platforms.

## Running locally

```bash
flox activate
task dev
```

The flox manifest doesn't include `cargo` or the Tauri CLI yet — add them once from the repo root with `flox install cargo cargo-tauri`.

## Building

```bash
cargo tauri build
```

The bundled app goes to `src-tauri/target/release/bundle/`. `task build` runs a plain `cargo build` (debug); use `cargo tauri build` for the release bundle.

## Project guide

See [AGENTS.md](AGENTS.md) for coding style, commit conventions, PR conventions, git hooks, and agent workflows.
