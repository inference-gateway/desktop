# AGENTS.md - Contributor Guide

## Project

Tauri v2 desktop client for Inference Gateway: a **React 19 + TypeScript** frontend (Bun + Vite, Tailwind CSS v4 + shadcn/ui) over a **Rust** backend. The cargo workspace holds `backend` (crate `inference-gateway-desktop`) and `e2e`. Run cargo from `backend/` or from the root with `-p inference-gateway-desktop`. The Taskfile wraps the common flows. Read `frontend/AGENTS.md` before changing the frontend: it holds the state layout and the DOM contract the e2e tests rely on. `docs/` holds the user-facing guide the README links - update the feature's page in the same change.

## Before You Start

```bash
git config core.hooksPath .githooks
```

Activate the pre-commit hook before making changes - it is inert until you do. It formats staged `.ts/.tsx` with prettier, checks final newlines, runs `bun run build` and `bun test`, then `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo check` and `cargo test` in `backend/`. The toolchain (rust, `cargo-tauri`, `task`, `bun`, `infer`) comes from the [flox manifest](.flox/env/manifest.toml). Enter it with `flox activate`.

## Commands

| Command | What it does |
| --- | --- |
| `task dev` | Run the app (`cargo tauri dev`) on the built `dist/`. After a frontend edit, `task web` rebuilds it and the window reloads |
| `task dev:hmr` | Run the app on the Vite dev server, so a frontend edit applies on save. It has no content security policy - check a change that loads a new kind of resource with `task dev` |
| `task web` | Build the frontend into `dist/` (`tsc && vite build`) |
| `task build` / `test` / `clippy` / `check` | The cargo step in `backend/`, building `dist/` first - `task test` runs `bun test` too |
| `task e2e -- tests/<name>.yaml` | Run the macOS e2e suite, optionally filtered to one test |
| `task e2e:record` | Run all e2e tests while recording (`artifacts/<slug>.mov` per test) |
| `bun test` | Frontend unit tests (`frontend/lib/*.test.ts`) |
| `cargo tauri build` | Release bundle. Needs `TAURI_SIGNING_PRIVATE_KEY` (see CONTRIBUTING) |
| `gh workflow run release.yml` | Cut a release: semantic-release tags it, the updater keys are stamped in, and the three OS bundles plus the `latest.json` updater manifest publish (see CONTRIBUTING) |
| `gh workflow run bump-cli.yml` | Open a draft pull request that moves `.cli-version` and `.cli-checksums.txt` to the latest CLI release, or to the one named with `-f version=v0.266.0`, once its checksums verify against the release signature |

The Rust build embeds `frontendDist` (`../dist`) via `generate_context!`, so **every path that compiles the crate builds `dist/` first**.

To click a local gateway build through the app, name it in `INFERENCE_GATEWAY_BIN`. The desktop runs that file in place of `~/.infer/bin/inference-gateway` and never installs over it, so an update or a reinstall leaves it alone. `INFER_BIN` does the same for the CLI:

```bash
INFERENCE_GATEWAY_BIN=/path/to/inference-gateway task dev
```

`.cli-version` names the CLI release this desktop is tested with, and `.cli-checksums.txt` holds that release's `checksums.txt`. A fresh install and **Update** take that release and refuse an archive whose SHA-256 the file does not list, and the e2e runs and CI's CLI contract check take it too. CI's `cli-release` job verifies the file against the release's cosign signature and the CLI release workflow's identity. The flox dev shell pins a separate `infer` in `.flox/env/manifest.toml`, so that pin can trail `.cli-version`. Move to a new release once the smoke set and the contract check pass on it. `gh workflow run bump-cli.yml` opens that pull request, and its CI runs both. By hand:

```bash
echo v0.265.0 > .cli-version
gh release download v0.265.0 --repo inference-gateway/cli -p checksums.txt -O .cli-checksums.txt --clobber
```

## Verifying the UI

`task e2e` runs the YAML tests in `e2e/tests/` against a fresh mock-mode build, driving the real UI through the macOS accessibility tree. `task e2e -- --podman` runs them in a Podman container against the Linux build instead, so a run leaves this machine's screen and mouse alone. How to run and write them, and how to record a demo, is in `e2e/AGENTS.md`. CI runs `bun run build` and `bun test` plus `cargo fmt --check`, `cargo clippy -- -D warnings` and `cargo test --workspace`, which validates the e2e test files without running them. It runs on ubuntu and windows for every pull request and on macOS for every push to `main`. A pull request builds on macOS and runs the e2e tests there only once it carries the `ci:macos` label, so add it when a change touches macOS-only code (`#[cfg(target_os = "macos")]`, the e2e harness) or the UI the tests drive. The `E2E` workflow runs the tests themselves. Every pull request and every push to `main` runs the whole suite on Linux, through the WebDriver driver the container run uses, and skips the tests marked `platform: macos`. A pull request with the label runs the smoke set in `SMOKE_SPECS` on macOS too, and the whole suite runs on macOS for every push to `main` and when started by hand (`gh workflow run e2e.yml`). A failed run uploads each test's log and failure screenshot as the `e2e-artifacts` artifact, or `e2e-linux-artifacts` for the Linux run. A markdown-only pull request skips the CI build matrix and the E2E workflow - the `markdown-lint` job lints its changed files instead.

## Window permissions (load-bearing)

Each window has its own capability file in `backend/capabilities/` and may only call the commands its permission in `backend/permissions/` allows. A new `#[tauri::command]` must be added to `permissions/main-window.toml` as well as to `generate_handler!` - `cargo test` fails until both lists match. Add it to `monitor-window.toml` only if the monitor bar calls it. The overlay calls no commands.

A command that is a plain `fn` runs on the main thread, and the window freezes while it works. Keep that work off it by marking the command `#[tauri::command(async)]`, or by writing it as an `async fn` that pushes the blocking part through `spawn_blocking`, `error::blocking` or `blocking_with_state` - `start_gateway` and `download_hub_model` show that shape. A small settings writer stays plain on purpose - the main thread keeps its writes in order. `write_projects` is the exception: the sidebar's save queue keeps its writes in order instead.

A command whose failure the UI handles differently returns `crate::error::Result` with a variant of its own in `backend/src/error.rs`. It reaches the frontend as a `CommandError` with that `kind` (parsed in `frontend/lib/tauri.ts`), which still prints as its message - never match on the wording of an error.

`cargo test` also pins event names: the kinds each window reads and the events `frontend/lib/events.ts` listens for must match what the backend emits (`backend/src/events.rs`), so a renamed event updates every listener in the same change.

The content security policy in `backend/tauri.conf.json` blocks inline and remote scripts, and loads remote images only from the two emoji repositories the agent editor uses. A new kind of remote resource (an image host, a font host, a media host) needs its directive widened there.

## Coding Style

- Rust: follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) and rustfmt. Prefer std over new dependencies - when adding one, use the latest stable version.
- Boring, explicit code over clever abstractions.
- Write self-explanatory code: clear names and small, single-purpose functions carry the intent.
  If a block needs a comment to be understood, extract it into a well-named function or variable.
- No inline comments inside function bodies.
- Doc comments on functions and types are at most 5 lines: what it does and why, not how.
- No comments above modules or files: no Rust `//!` crate or module docs, no TypeScript
  `@fileoverview` or `@module` block, no file header.
- Document a struct, class, or record once above its declaration, not on each member. If a field
  needs explaining, rename it or fold the explanation into the type's doc comment.
- Prefer named constants or enums over magic numbers and strings. A bare literal is fine only
  where its meaning is obvious at a glance (0, 1, an empty string).
- Tool directives are not comments and stay where the tool needs them (lint suppressions, build
  tags, compiler pragmas, code generation markers).
- No semicolons in documentation prose (Markdown files, doc comments, e2e test names and
  narration): split the sentence or use a dash instead.
- Examples (`examples/` directories, README snippets) are short and focused: one feature per
  example, the fewest lines that show it, and the exact command to run it.
- Every example must run as written. Verify it against the `tokenless` mock gateway, or against a
  real model such as `ollama_cloud/glm-5.3-flash` when the mock cannot cover it. Fix or delete an
  example that does not run.
- User-facing docs (README, CONTRIBUTING) use `-`, not em dashes.
- No issue numbers in documentation, doc comments or examples.
- YAML: block lists (`- item` per line), never flow arrays (`[a, b]`).
- GitHub Actions: pin every `uses:` to an explicit release tag (`v2.9.2`, not `v2`) or a commit SHA.

## Commits & PRs

Conventional Commits: `<type>(<scope>): <description>`. Types `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`, `ci`. Keep commits atomic. Open PRs as **drafts** early, titled in the same format, body containing `## Summary` and `## Changes`. A human reviews and merges - never self-merge. One concern per PR.

## Agent Workflows

AI tasks run from three workflows: `tasks.yml` (`@opentask`, `inference-gateway/infer-action` with the **documentation-agent** A2A agent and the plugins `DietrichGebert/ponytail` and `ayghri/i-have-adhd`), `infer.yml` (`@infer`, the org's shared Infer workflow with the `DietrichGebert/ponytail` plugin) and `claude.yml` (`@claude`).

When a GitHub issue belongs to a project board, keep its Status in sync - best-effort, never abort the task over a board error. Detect membership with `gh issue view <number> --json projectItems` (never by scanning the board), edit with `gh project item-add` (idempotent) and `gh project item-edit`. **Todo -> In Progress** when starting, **QA** after the PR opens, never **Done** - that happens at merge.
