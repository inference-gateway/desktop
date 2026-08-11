# Contributing

## Prerequisites

- **Flox** - the dev environment (Rust 1.95 toolchain, `task`, `bun`, `infer`) is defined in the [flox manifest](.flox/env/manifest.toml). Install [flox](https://flox.dev), then run `flox activate` from the repo root.
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

The flox manifest doesn't include `cargo` or the Tauri CLI yet - add them once from the repo root with `flox install cargo cargo-tauri`.

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

## Project guide

See [AGENTS.md](AGENTS.md) for coding style, commit conventions, PR conventions, git hooks, and agent workflows.
