# Moving to a new machine

Settings > General > **Export / Import** moves the portable part of the desktop state between machines: the settings stored in `~/.infer/config.yaml` (the fields of `DesktopConfig` in `backend/src/config.rs`), sidebar projects, A2A agents, scheduled jobs, snippets, the skills registry URL and installed skills - the list `build_export` in `backend/src/export.rs` collects. Everything below does not travel with the file, so plan to redo it on the new machine.

Export writes one portable file (JSON, YAML or TOML) in a native save dialog, or pushes it to a private GitHub repo you name (created on demand, public repos are refused). Import reads it back from either place and auto-detects the format.

## What the export leaves behind

The export file (`DesktopExport` in `backend/src/export.rs`) has no field for these: your own Markdown agents (`~/.infer/agents`), workspaces (`~/.infer/daemon.yaml`), clusters (`~/.infer/compute.yaml`), the local cluster's folders and runtime (`~/.infer/cluster_folders.yaml`), MCP servers (`~/.infer/mcp.yaml`), secrets, voice samples, avatars, downloaded models, and the Computer Use and Browser Use switches (`~/.infer/computer_use.yaml`, `~/.infer/browser_use.yaml`). Two Settings > General fields live only in the webview's local storage and are not exported either: Max concurrent sessions (`maxConcurrentSessions` in `frontend/store.tsx`) and Screen recordings to keep (`screenRecordKeep` in `frontend/lib/tauri.ts`). Copy what you need by hand before switching machines.

## Credentials

Credentials are never exported: `scrub_credentials` in `backend/src/export.rs` blanks the database passwords and tokens and, along with them, the scheduler's two GitHub App secret names (`auth.yaml` keys are never read for the export at all). The file carries those fields blank, and an import keeps the values already on the new machine.

Machine-specific paths are stored `~/`-relative so they resolve against the new machine's home. Projects that are GitHub checkouts travel as their `owner/name` rather than a full copy: Import re-clones them under the projects root and re-reads their `AGENTS.md`, keeping the file small (edited project instructions are still carried in full).

