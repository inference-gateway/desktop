# AGENTS.md - Contributor Guide

## Project

Tauri v2 desktop client for Inference Gateway: a **React 19 + TypeScript** frontend (Bun + Vite, Tailwind CSS v4 + shadcn/ui) over a **Rust** backend. The cargo workspace holds `backend` (crate `inference-gateway-desktop`) and `e2e`; run cargo from `backend/` or from the root with `-p inference-gateway-desktop`. The Taskfile wraps the common flows.

## Before You Start

```bash
git config core.hooksPath .githooks
```

Activate the pre-commit hook before making changes - it is inert until you do. The hook prettier-formats staged `.ts/.tsx`, enforces a final newline on tracked text files, runs the frontend build (`bun run build`) and unit tests (`bun test`), then `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo check`, `cargo test` in `backend/`. The toolchain (rust, `cargo-tauri`, `task`, `bun`, `infer`) comes from the [flox manifest](.flox/env/manifest.toml); enter it with `flox activate`.

## Commands

| Command | What it does |
| --- | --- |
| `task dev` | Run the app (`cargo tauri dev`). No frontend hot-reload - re-run `task web` after frontend edits |
| `task web` | Build the frontend into `dist/` (`tsc && vite build`) |
| `task build` / `test` / `clippy` / `check` | The cargo step in `backend/`, building `dist/` first - `task test` runs `bun test` too |
| `task e2e -- tests/<name>.yaml` | Run the macOS e2e suite, optionally filtered to one test |
| `task e2e:record` | Run all e2e tests while recording (`artifacts/<slug>.mov` per test) |
| `bun test` | Frontend unit tests (`frontend/lib/*.test.ts`) |
| `cargo tauri build` | Release bundle; needs `TAURI_SIGNING_PRIVATE_KEY` (see CONTRIBUTING) |

The Rust build embeds `frontendDist` (`../dist`) via `generate_context!`, so **every path that compiles the crate builds `dist/` first**.

## Verifying the UI

`task e2e` runs the YAML tests in `e2e/tests/` against a fresh mock-mode build, driving the real UI through the macOS accessibility tree - no WebDriver, no `tauri-driver`. Every test file demos exactly one feature and reads top to bottom as a walkthrough: `name:` states the behaviour, `narration:` carries one line per step that the runner prints as it goes, and `steps:` drives the UI. CI (ubuntu) runs `bun run build` and `bun test` plus `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` - no e2e.

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

Test-file extras: `record: true` wraps the run in `screencapture -v` and writes `artifacts/<slug>.mov` (no editing, no captions - that is what the app's own timeline tooling is for); the runner's `--record` flag (`task e2e:record`) forces recording on every test, ignoring the per-file flag, so all e2e tests run while recording; missing `narration:` entries fall back to the mechanical step label, and `cargo test -p desktop-e2e` rejects duplicate test names and narration lists longer than the step list. The remaining `DESKTOP_MOCK` fakes (permissions, stt, timeline, screen records, scheduler) are OS/tool shims, not LLM shims.

Demo recordings (`@infer /demo` or `@opentask /demo`, Linux CI): the `demo` skill records the Xvfb display `:99` (1280x720, no window manager). Run the build in the `demo` tmux session - a cold build takes about 2.5 minutes, longer than the 120 s Bash timeout - and use the binary, since `cargo-tauri` is not installed there. The app needs 20-30 s to show its window on the runner (GTK waits for an accessibility bus that is not there) and longer while recording, so rehearse and start the take only once the window is up. The 760px-tall main window overflows the display: wait for it to be visible, resize it, and repeat the resize once the UI has rendered, because `center: true` can move it back. Send only prompts that a `scenarios.yaml` scenario answers:

```bash
bun run build
cargo build -p inference-gateway-desktop
DISPLAY=:99 WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 LIBGL_ALWAYS_SOFTWARE=1 DESKTOP_MOCK=true INFER_GATEWAY_MOCK_SCENARIOS=$PWD/e2e/scenarios.yaml ./target/debug/inference-gateway-desktop &
DISPLAY=:99 xdotool search --onlyvisible --sync --name '^Inference Gateway Desktop$' windowmove 0 0 windowsize 1280 720
```

## DOM contract (load-bearing)

The e2e harness drives the real UI through the accessibility tree. When editing `frontend/components/`, preserve:

- Native `<select id="model-select">` (not a custom dropdown).
- A single native, **uncontrolled** `<textarea id="prompt-input">`.
- Exact button names: visible text `Approve`, `Deny`, `Save agent`, `New agent`, `Take over`, `Hand back`, `New workspace`, `Save workspace`, `New cluster`, `Save cluster`, `Detect clusters`, `New secret`, `Save secret`, `Save inbox key as secret`, `New hosted server`, `New container server`, `Save server`, `Add header`; `aria-label` on icon buttons (`New chat`, `Broadcast to projects`, `Filter all`, `Filter agents`, `Send`, `Restart CLI`, `Settings`, `Computers`, `Voice input`, `Stop`, `Delete conversation`, `Move to workspace`, `Clean up machines`, `Artifacts`, `Reload artifacts`, `View image`, `Choose agent project`, `Provision local cluster`) and on per-item buttons (`Select project <name>`, `Leave project <name>`, `Select agent <name>`, `Leave agent <name>`, `Edit agent <name>`, `Delete agent <name>`, `Open computer <name>`, `Delete computer <name>`, `Move to <workspace>` and `Move to this computer`, `Check workspace <name>`, `Edit workspace <name>`, `Delete workspace <name>`, `Delete machine <name>`, `Check cluster <name>`, `Edit cluster <name>`, `Delete cluster <name>`, `Edit secret <label>`, `Delete secret <label>`, `Check MCP servers`, `Edit server <name>`, `Delete server <name>`, `Remove header <n>`, `Save artifact <name>`, `View shell <command>`, `Use project <name>` and `No project`).
- Workspace form inputs in Settings keep their ids (`workspace-compute`, `workspace-kind`, `workspace-name`, `workspace-context`, `workspace-namespace`, `workspace-image`, `workspace-user-data`); Enter in the name, context or namespace field moves focus to the next one, and on a workspace with a cluster the name field moves to the image.
- Cluster form inputs in Settings > Compute keep their ids (`cluster-name`, `cluster-context`, `cluster-namespace`, `cluster-gateway-env`); Enter in the name, context or namespace field moves focus to the next one. **Detect clusters** and **Provision local cluster** live on the Compute tab.
- Secret form inputs in Settings > Secrets keep their ids (`secret-label`, `secret-variable`, `secret-value`, `secret-computer`, `secret-workspace-<name>`). Enter in the label or variable field moves focus to the next one.
- MCP server form inputs in Settings > MCPs keep their ids (`mcp-name`, `mcp-url`, `mcp-image`, `mcp-port`, `mcp-computer`, `mcp-workspace-<name>`, and per header `mcp-header-name-<n>`, `mcp-header-prefix-<n>`, `mcp-header-variable-<n>`). The global switch is `mcp-enabled`. Enter in the name field moves focus to the URL or the image, and in the image to the port.
- Agent editor inputs keep their ids (`agent-name`, `agent-description`, `agent-model`, `agent-workspace`, `agent-project`, `agent-instructions`, `agent-emoji`, and `agent-mcp-<server>` per MCP server); Enter in the name or description field moves focus to the next one, which is how the e2e `type` step fills the form.
- Shallow DOM: `App` renders `<header id="top-bar">` + `<div id="main">` into `#app`, no wrapper.

## Window permissions (load-bearing)

Each window has its own capability file in `backend/capabilities/` and may only call the commands its permission in `backend/permissions/` allows. A new `#[tauri::command]` must be added to `permissions/main-window.toml` as well as to `generate_handler!` - `cargo test` fails until both lists match. Add it to `monitor-window.toml` only if the monitor bar calls it. The overlay calls no commands.

The content security policy in `backend/tauri.conf.json` blocks inline and remote scripts. A new kind of remote resource (a font host, a media host) needs its directive widened there.

State lives in context stores: the main one in `frontend/store.tsx` (`useDesktop`) and one per feature in `frontend/stores/` (`useActivity`, `useSnippets`, `useStatus`), each with its own context so its readers re-render only when that feature changes. New feature state goes in `frontend/stores/`, not in the main store. The typed Tauri client and transcript state machine live in `frontend/lib/`.

## Coding Style

- Rust: follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) and rustfmt. Prefer std over new dependencies - when adding one, use the latest stable version.
- Boring, explicit code over clever abstractions.
- Write self-explanatory code: clear names and small, single-purpose functions carry the intent.
  If a block needs a comment to be understood, extract it into a well-named function or variable.
- No inline comments inside function bodies.
- Doc comments on functions and types are at most 5 lines: what it does and why, not how.
- No comments above modules, packages, or files.
- Tool directives are not comments and stay where the tool needs them (lint suppressions, build
  tags, compiler pragmas, code generation markers).
- No semicolons in documentation prose (Markdown files, doc comments): split the sentence or use
  a dash instead.
- User-facing docs (README, CONTRIBUTING) use `-`, not em dashes.
- YAML: block lists (`- item` per line), never flow arrays (`[a, b]`).
- GitHub Actions: pin every `uses:` to an explicit release tag (`v2.9.2`, not `v2`) or a commit SHA.

## Commits & PRs

Conventional Commits: `<type>(<scope>): <description>`; types `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `ci`. Keep commits atomic. Open PRs as **drafts** early, titled in the same format, body containing `## Summary` and `## Changes`. A human reviews and merges - never self-merge. One concern per PR.

## Agent Workflows

AI tasks run via `inference-gateway/infer-action` in `tasks.yml` (triggers: `workflow_dispatch`, `issues`, `issue_comment`, `pull_request_review_comment`; phrase: `@opentask`), defaulting to the **documentation-agent** A2A agent, with plugins `DietrichGebert/ponytail` and `ayghri/i-have-adhd`.

When a GitHub issue belongs to a project board, keep its Status in sync - best-effort, never abort the task over a board error. Detect membership with `gh issue view <number> --json projectItems` (never by scanning the board), edit with `gh project item-add` (idempotent) and `gh project item-edit`. **Todo -> In Progress** when starting, **QA** after the PR opens, never **Done** - that happens at merge.
