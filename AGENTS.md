# AGENTS.md - Contributor Guide

## Project Structure

The repository is a scaffold for agent-driven development on `inference-gateway/desktop`. It pairs a **React + TypeScript** frontend (Bun + Vite, Tailwind CSS v4 + shadcn/ui) with a **Tauri v2 / Rust** backend. Cargo commands run from `backend/` (or the workspace root with `-p inference-gateway-desktop`); the Taskfile wraps them at the repo root and builds the frontend first where needed.

## Before You Start

Activate the pre-commit hook before making any changes:

```bash
git config core.hooksPath .githooks
```

The hook checks that every tracked text file ends with a final newline (`.editorconfig`), then runs `cargo fmt --check`, `cargo clippy`, `cargo check`, and `cargo test` on every commit. It is **inert until you run the above command** - the committed file is a script, not an active hook.

## Build / Test / Dev Commands

The dev environment - Rust toolchain, `cargo`, `cargo-tauri`, `task`, `bun`, `infer` - is provided by the [flox manifest](.flox/env/manifest.toml); enter it with `flox activate`.

The Rust build embeds the frontend from `frontendDist` (`../dist`) via `generate_context!`, so **every path that compiles the crate first builds `dist/`**. Frontend unit tests run with `bun test`.

## Frontend (React + TypeScript)

Frontend source lives in `frontend/`; state is a single context store (`frontend/store.tsx`); the typed Tauri client and transcript state machine are in `frontend/lib/`.

**The e2e harness drives the real UI through the macOS accessibility tree, so the DOM contract is load-bearing** - preserve these when editing components:

- The model picker stays a native `<select id="model-select">` (not a custom dropdown).
- The composer stays a single native, **uncontrolled** `<textarea id="prompt-input">`.
- Keep exact button names: visible text `Approve`, `Deny`; `aria-label` on icon buttons (`New chat`, `Broadcast to projects`, `Send`, `Restart CLI`, `Settings`, `Voice input`, `Stop`, `Delete conversation`).
- Keep the DOM shallow: `App` renders `<header id="top-bar">` + `<div id="main">` directly into `#app` (no wrapper).

## Verifying the UI

**`task e2e` runs YAML-defined tests** in `e2e/tests/` against a fresh mock-mode build, driving the real UI through the macOS accessibility tree - launch, type, click, approve, assert on disk - with zero tokens. Use `DESKTOP_MOCK=true` for manual mock-mode sessions. See `e2e/` source and `e2e/scenarios.yaml` for test definitions and canned LLM turns.

On CI (ubuntu), the app is built with `bun run build` and Rust checks run (fmt, clippy, test) - no macOS e2e tests. macOS-only e2e uses AX element actions (no WebDriver), so `tauri-driver` is not required.

## Coding Style

- **Language**: Rust. Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) and standard `rustfmt` style.
- **Simplicity**: Prefer the standard library over external dependencies. Favor boring, explicit code over clever abstractions.
- **Dependencies**: Look up the latest stable version (`cargo search` / `cargo info`) and use it - never pin an older version.
- **`ponytail:` comments**: Mark deliberate shortcuts with a `ponytail:` comment naming the ceiling and upgrade path (e.g. `ponytail: O(n^2) - fine for <100 items`).
- **No inline comments in function bodies**: Code should be self-documenting. Use inline `//` comments only at the module level or as `ponytail:` debt markers.
- **User-facing text uses regular dashes**: Use `-` (regular dash) instead of em dashes in README, CONTRIBUTING, and other user-facing docs. Em dashes are reserved for internal/agent-facing files.

## Commit Conventions

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>
```

**Types**: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `ci`

Examples:
- `feat(api): add rate-limit header`
- `fix(web): handle empty state in user list`
- `docs: update README with setup instructions`
- `chore(deps): bump lodash to 4.17.21`

Keep commits atomic - one logical change per commit.

## Pull Request Conventions

- Open PRs as **drafts** early, even with partial work.
- PR title follows the same Conventional Commit format.
- PR body must include `## Summary` and `## Changes` sections.
- A human reviews and merges - do not self-merge.
- Keep the PR focused on a single concern.

## Agent Workflows

This repository uses `inference-gateway/infer-action` for AI-assisted development. The CI workflow (`tasks.yml`) triggers on `workflow_dispatch`, `issues`, `issue_comment`, and `pull_request_review_comment`. Trigger phrase: `@opentask`.

### Project Board Management

When working on a GitHub issue that belongs to a project board, keep its status in sync. Detect board membership from `gh issue view <number> --json projectItems` (not by scanning the board).

1. **Start of work**: Move the issue from **Todo** to **In Progress**.
2. **Completion**: Move to **QA** (never **Done** - that is a human step after QA).
3. Use `gh project item-add` (idempotent) and `gh project item-edit` to update the Status field.

### Available Agents
- **documentation-agent**: Documentation generation and updates.

### Plugins
- **ponytail** (`DietrichGebert/ponytail`): Lazy senior dev mode - forces the simplest, most minimal solution.
- **i-have-adhd** (`ayghri/i-have-adhd`): ADHD-friendly workflow support.
