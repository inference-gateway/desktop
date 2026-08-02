# AGENTS.md — Contributor Guide

## Project Structure

```
.
├── .github/workflows/tasks.yml   # CI/CD: agent-driven workflow (infer-action)
├── README.md                     # Project overview
├── AGENTS.md                     # This file — contributor & agent guide
├── CLAUDE.md → AGENTS.md         # Symlink for Claude Code compatibility
├── .githooks/pre-commit          # Pre-commit hook (typecheck + tests)
├── .flox/env/                    # Flox dev environment (pinned Rust toolchain, task, infer)
├── Taskfile.yml                  # Task runner: common build/test/dev commands
├── .agents/skills/               # Agent skill definitions
├── .claude/skills → ../.agents/skills  # Symlink for Claude Code
├── src/                          # Static frontend (HTML/CSS/JS)
│   ├── index.html
│   ├── main.js
│   └── style.css
└── src-tauri/                    # Tauri v2 backend
    ├── Cargo.toml
    ├── build.rs
    ├── tauri.conf.json
    ├── capabilities/
    │   └── default.json
    └── src/
        ├── lib.rs
        └── main.rs
```

The repository is a **scaffold for agent-driven development** on `inference-gateway/desktop`. It is intentionally minimal — the CI workflow (`tasks.yml`) is the primary entry point for automated work.

## Build / Test / Dev Commands

This project uses [Cargo](https://doc.rust-lang.org/cargo/) (Rust's build tool and package manager). The dev environment — a pinned Rust 1.95 toolchain plus `task` and `infer` — is provided by the [flox manifest](.flox/env/manifest.toml); enter it with `flox activate`. Cargo commands run from `src-tauri/`; the [Taskfile](Taskfile.yml) wraps them at the repo root.

| Task            | Description                                                          |
|-----------------|----------------------------------------------------------------------|
| `task build`    | Build the project (`cargo build`)                                  |
| `task test`     | Run all tests (`cargo test`)                                       |
| `task clippy`   | Lint and format check (`cargo fmt --check`, `cargo clippy -- -D warnings`) |
| `task check`    | Typecheck, no codegen (`cargo check`)                              |
| `task dev`      | Start dev server with hot-reload (`cargo tauri dev`)               |

## Coding Style

- **Language**: Rust. Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) and standard style via `rustfmt`.
- **Formatting**: `cargo fmt` — no debates.
- **Simplicity**: Prefer the standard library over external dependencies. Favor boring, explicit code over clever abstractions.
- **Dependencies**: When adding a dependency, look up the latest stable version first (`cargo search <name>` / `cargo info <name>`) and use it — never pin an older version copied from another project or tutorial.
- **YAGNI**: Don't add code until it's needed. Delete dead code when you find it.
- **Error handling**: Handle errors at trust boundaries. Don't swallow errors silently.
- **`ponytail:` comments**: Mark deliberate shortcuts with a `ponytail:` comment naming the ceiling and upgrade path (e.g. `ponytail: O(n²) — fine for <100 items`).
- **No inline comments in function bodies**: The code should be self-documenting. Inline `//` comments inside function bodies are not allowed — use them only at the module level or as `ponytail:` debt markers.
- **User-facing text uses regular dashes**: Use `-` (regular dash) instead of `—` (em dash) in README.md, CONTRIBUTING.md, and any other user-facing documentation. Em dashes are reserved for internal/agent-facing files like this one.

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

Keep commits atomic — one logical change per commit.

## Pull Request Conventions

- Open PRs as **drafts** early, even with partial work.
- PR title follows the same Conventional Commit format.
- PR body must include `## Summary` and `## Changes` sections.
- A human reviews and merges — do not self-merge.
- Keep the PR focused on a single concern.

## Git Hooks

This repository ships a pre-commit hook at `.githooks/pre-commit`. To activate it:

```bash
git config core.hooksPath .githooks
```

The hook runs typecheck and tests before each commit. It is **inert until you run the above command** — the committed file is a script, not an active hook.

## Agent Workflows

This repository uses `inference-gateway/infer-action` for AI-assisted development. The CI workflow (`tasks.yml`) triggers on:

- **`workflow_dispatch`**: Manual dispatch with a prompt.
- **`issues`**: New or edited issues.
- **`issue_comment`**: Comments on issues.
- **`pull_request_review_comment`**: PR review comments.

Trigger phrase for the agent: `@opentask`

### Project Board Management

When working on a GitHub issue that belongs to a project board, keep the board's status in sync:

1. **Start of work**: Move the issue from **Todo** to **In Progress** before making any changes.
2. **Completion**: When the work is done (PR opened, changes committed, or comment posted), move the issue to **QA**.
3. **Never Done**: Issues are never moved to **Done** — that transition is reserved for human review after QA sign-off.

Use `gh project item-add` to add the issue to the board (idempotent — returns the existing item if already present) and `gh project item-edit` to update its status field. Detect board membership from the issue's `projectItems` field (`gh issue view <number> --json projectItems`) rather than scanning the board.

### Available Agents

- **documentation-agent**: Handles documentation generation and updates.

### Plugins

- **ponytail** (`DietrichGebert/ponytail`): Lazy senior dev mode — forces the simplest, most minimal solution.
- **i-have-adhd** (`ayghri/i-have-adhd`): ADHD-friendly workflow support.

## Activating the Project Locally

```bash
# Clone
git clone git@github.com:inference-gateway/desktop.git
cd desktop

# Enter the flox dev environment (Rust 1.95 toolchain, task, infer)
flox activate

# Activate git hooks
git config core.hooksPath .githooks

# Run checks (or the cargo equivalents from src-tauri/)
task check
```
