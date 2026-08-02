# AGENTS.md — Contributor Guide

## Project Structure

```
.
├── .github/workflows/tasks.yml   # CI/CD: agent-driven workflow (infer-action)
├── README.md                     # Project overview
├── AGENTS.md                     # This file — contributor & agent guide
├── CLAUDE.md → AGENTS.md         # Symlink for Claude Code compatibility
├── .githooks/pre-commit          # Pre-commit hook (typecheck + tests)
├── .agents/skills/               # Agent skill definitions
├── .claude/skills → ../.agents/skills  # Symlink for Claude Code
└── Taskfile.yml                  # Task runner (Go-based task runner)
```

The repository is a **scaffold for agent-driven development** on `inference-gateway/desktop`. It is intentionally minimal — the CI workflow (`tasks.yml`) is the primary entry point for automated work.

## Build / Test / Dev Commands

This project uses [Task](https://taskfile.dev) (Go-based task runner) for all development commands. Install it with:

```bash
go install github.com/go-task/task/v3/cmd/task@latest
# or via package manager: brew install go-task/tap/go-task
```

Common commands (once a `Taskfile.yml` is present):

| Command       | Description                     |
|---------------|---------------------------------|
| `task build`  | Build the project               |
| `task test`   | Run all tests                   |
| `task lint`   | Lint and format check           |
| `task check`  | Typecheck + lint + test (CI)   |
| `task dev`    | Start dev server / watch mode   |

Until a `Taskfile.yml` is added, run checks directly:

```bash
# Typecheck (if applicable)
# e.g. tsc --noEmit, pyright, etc.

# Tests
# e.g. go test ./..., pytest, etc.
```

## Coding Style

- **Language**: Follow the standard style for the language used (Go, TypeScript, Python, etc.).
- **Formatting**: Use the project's formatter (gofmt, prettier, black, etc.) — no debates.
- **Simplicity**: Prefer the standard library over external dependencies. Favor boring, explicit code over clever abstractions.
- **YAGNI**: Don't add code until it's needed. Delete dead code when you find it.
- **Error handling**: Handle errors at trust boundaries. Don't swallow errors silently.
- **`ponytail:` comments**: Mark deliberate shortcuts with a `ponytail:` comment naming the ceiling and upgrade path (e.g. `ponytail: O(n²) — fine for <100 items`).

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

# Install Task (if not present)
go install github.com/go-task/task/v3/cmd/task@latest

# Activate git hooks
git config core.hooksPath .githooks

# Run checks
task check   # or the equivalent direct commands
```
