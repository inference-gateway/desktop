# AGENTS.md - Contributor Guide

## Project

Tauri v2 desktop client for Inference Gateway: a **React 19 + TypeScript** frontend (Bun + Vite, Tailwind CSS v4 + shadcn/ui) over a **Rust** backend. The cargo workspace holds `backend` (crate `inference-gateway-desktop`) and `e2e`; run cargo from `backend/` or from the root with `-p inference-gateway-desktop`. The Taskfile wraps the common flows.

## Before You Start

```bash
git config core.hooksPath .githooks
```

Activate the pre-commit hook before making changes - it is inert until you do. The hook prettier-formats staged `.ts/.tsx`, enforces a final newline on tracked text files, runs the frontend build (`bun run build`), then `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo check`, `cargo test` in `backend/`. The toolchain (rust, `cargo-tauri`, `task`, `bun`, `infer`) comes from the [flox manifest](.flox/env/manifest.toml); enter it with `flox activate`.

## Commands

| Command | What it does |
| --- | --- |
| `task dev` | Run the app (`cargo tauri dev`). No frontend hot-reload - re-run `task web` after frontend edits |
| `task web` | Build the frontend into `dist/` (`tsc && vite build`) |
| `task build` / `test` / `clippy` / `check` | The cargo step in `backend/`, building `dist/` first |
| `task e2e -- tests/<name>.yaml` | Run the macOS e2e suite, optionally filtered to one test |
| `bun test` | Frontend unit tests (`frontend/lib/*.test.ts`) |
| `cargo tauri build` | Release bundle; needs `TAURI_SIGNING_PRIVATE_KEY` (see CONTRIBUTING) |

The Rust build embeds `frontendDist` (`../dist`) via `generate_context!`, so **every path that compiles the crate builds `dist/` first**.

## Verifying the UI

`task e2e` runs the YAML tests in `e2e/tests/` against a fresh mock-mode build, driving the real UI through the macOS accessibility tree - no WebDriver, no `tauri-driver`. Every test file demos exactly one feature and reads top to bottom as a walkthrough: `name:` states the behaviour, `narration:` carries one line per step that the runner prints as it goes, and `steps:` drives the UI. CI (ubuntu) runs `bun run build` plus `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` - no e2e.

Running the tests (macOS only):

```bash
task e2e -- tests/approval-approve.yaml   # one feature
task e2e                                  # all features, alphabetical
cargo test -p desktop-e2e                 # parse/validate the YAML only (what CI does)
```

Every model turn is tokenless: the runner launches the app with `DESKTOP_MOCK=true` and `INFER_GATEWAY_MOCK_SCENARIOS=e2e/scenarios.yaml`, and the spawned `infer` children serve those scripted scenarios - nothing hits a real provider. A prompt with no matching scenario gets the `fallback` answer, so a test prompt must only promise what its scenario's turns produce. The `models:` block at the top of `scenarios.yaml` is the single source of truth for the mock-mode model list (the app reads it instead of hitting a gateway), so manual mock mode needs the same env var for both turns and models:

```bash
DESKTOP_MOCK=true INFER_GATEWAY_MOCK_SCENARIOS=e2e/scenarios.yaml task dev
```

Test-file extras: `record: true` wraps the run in `screencapture -v` and writes `artifacts/<slug>.mov` (no editing, no captions - that is what the app's own timeline tooling is for); missing `narration:` entries fall back to the mechanical step label, and `cargo test -p desktop-e2e` rejects duplicate test names and narration lists longer than the step list. The remaining `DESKTOP_MOCK` fakes (permissions, stt, timeline, screen records, scheduler) are OS/tool shims, not LLM shims.

## DOM contract (load-bearing)

The e2e harness drives the real UI through the accessibility tree. When editing `frontend/components/`, preserve:

- Native `<select id="model-select">` (not a custom dropdown).
- A single native, **uncontrolled** `<textarea id="prompt-input">`.
- Exact button names: visible text `Approve`, `Deny`; `aria-label` on icon buttons (`New chat`, `Broadcast to projects`, `Send`, `Restart CLI`, `Settings`, `Voice input`, `Stop`, `Delete conversation`).
- Shallow DOM: `App` renders `<header id="top-bar">` + `<div id="main">` into `#app`, no wrapper.

State is a single context store (`frontend/store.tsx`); the typed Tauri client and transcript state machine live in `frontend/lib/`.

## Coding Style

- Rust: follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) and rustfmt. Prefer std over new dependencies; when adding one, use the latest stable version.
- Boring, explicit code over clever abstractions. No inline comments in function bodies - module-level or `ponytail:` debt markers only; the marker names the ceiling and upgrade path (e.g. `ponytail: O(n^2) - fine for <100 items`).
- User-facing docs (README, CONTRIBUTING) use `-`, not em dashes.

## Commits & PRs

Conventional Commits: `<type>(<scope>): <description>`; types `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `ci`. Keep commits atomic. Open PRs as **drafts** early, titled in the same format, body containing `## Summary` and `## Changes`. A human reviews and merges - never self-merge. One concern per PR.

## Agent Workflows

AI tasks run via `inference-gateway/infer-action` in `tasks.yml` (triggers: `workflow_dispatch`, `issues`, `issue_comment`, `pull_request_review_comment`; phrase: `@opentask`), defaulting to the **documentation-agent** A2A agent, with plugins `DietrichGebert/ponytail` and `ayghri/i-have-adhd`.

When a GitHub issue belongs to a project board, keep its Status in sync - best-effort, never abort the task over a board error. Detect membership with `gh issue view <number> --json projectItems` (never by scanning the board), edit with `gh project item-add` (idempotent) and `gh project item-edit`. **Todo -> In Progress** when starting, **QA** after the PR opens, never **Done** - that happens at merge.
