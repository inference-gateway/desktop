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

## Verifying the UI

**`task e2e` is the way to test features.** It runs the YAML-defined tests in
`e2e/tests/` against a fresh mock-mode build of the app, driving the real UI
through the macOS accessibility tree — launch, type, click, approve, assert on
disk — with zero tokens. Add a test by copying a YAML in `e2e/tests/`; add a
mock scenario (canned LLM turns, incl. tool calls) in `e2e/scenarios.yaml`.
`task e2e -- tests/foo.yaml` runs one file; `--no-build` skips the app build;
`INFER_BIN=<path>` points the app at a specific infer build. Failures leave a
window screenshot and the app log in `e2e/artifacts/`. Runs leave their test
conversations in the user-level infer history (harmless; delete from the
sidebar if they bother you). macOS-only; CI (ubuntu) runs only fmt/clippy/test —
no `package.json`, no Playwright, no WebdriverIO, no `tauri-driver`.

The sections below document the raw AX primitives the harness is built on —
use them when a test fails and you need to poke the tree by hand.

### Launch and screenshot

```bash
# 1. Launch in the background, then poll the log until it prints "Running target/debug/..."
flox activate -- cargo tauri dev > /tmp/tauri-dev.log 2>&1 &

# 2. Capture BY WINDOW ID, not screen rect — `screencapture -R` grabs whatever
#    Space is currently visible, including the user's other windows. Find the id:
swift - <<'EOF'
import CoreGraphics
let info = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as! [[String: Any]]
for w in info where (w["kCGWindowOwnerName"] as? String ?? "").contains("inference-gateway") {
    if let b = w["kCGWindowBounds"] as? [String: Int], b["Height"]! > 100 { print(w["kCGWindowNumber"]!) }
}
EOF

# 3. Capture just that window
screencapture -x -l <window-id> /tmp/app.png
```

Frontend files are not hot-reloaded — `cargo tauri dev` watches `src-tauri/` only.
After editing `src/`, kill the process and relaunch.

### Clicking and typing work — via AX element actions only

`keystroke` and `click at {x,y}` do NOT work here, but not for permission reasons:
they target the *frontmost* app, and the dev build is a bare unbundled binary
(no bundle identifier), so macOS cooperative activation refuses to focus it and
the keystrokes land in the invoking terminal instead. The fix is to skip focus
entirely: WKWebView exposes a complete AX tree (text area, every button, the
transcript), and System Events **AX element actions** are delivered to the element
regardless of which app is frontmost. Requires the terminal to hold Accessibility
permission (check: `AXIsProcessTrusted` — keystroke landing in the terminal
proves it's granted).

```applescript
tell application "System Events" to tell (first process whose name contains "inference-gateway-desktop")
	set root to UI element 1 of scroll area 1 of group 1 of group 1 of window 1
	set value of text area 1 of root to "Create a file named test.txt with content hello using the Write tool"
	click button "Send" of root
end tell
```

Buttons (`Send`, `Approve`, `Deny`, `+ New chat`, `Delete conversation`, …) are
clickable the same way. Setting AXValue on the textarea propagates to the DOM —
Send reads the real value.

Two verified quirks:

- **AXValue only sticks on a fresh instance after one AX click inside the
  webview.** After hot rebuilds the set silently no-ops (read back "" every
  time). Recipe that always works: kill + relaunch, then `click button
  "+ New chat"` first, then set the value — and read it back to confirm before
  clicking Send.
- **`entire contents of window 1` is flaky** (intermittently returns nothing or
  errors -1700 when filtered). For finding a button, walk `UI elements`
  recursively with a handler instead:

```applescript
on findButton(el, btnName, depth)
	tell application "System Events"
		if depth > 8 then return missing value
		try
			if role of el is "AXButton" and name of el is btnName then return el
		end try
		try
			repeat with c in (UI elements of el)
				set r to my findButton(c, btnName, depth + 1)
				if r is not missing value then return r
			end repeat
		end try
		return missing value
	end tell
end findButton
```

### e2e with a real model

Always suggest **`deepseek/deepseek-v4-flash`** — the cheapest model available
through the gateway. Full approval-flow check: send a Write-triggering prompt,
wait for the "Tool requires approval" card, AX-click **Approve**, assert the file
exists on disk (the agent's cwd is `src-tauri/` in dev — relative tool paths land
there), then delete it. Repeat with **Deny** and assert the file was not written.

### e2e without tokens: DESKTOP_MOCK=true

```bash
DESKTOP_MOCK=true flox activate -- cargo tauri dev
```

`DESKTOP_MOCK=true` (see `mock_mode()` in `src-tauri/src/lib.rs`) makes the
desktop skip its own gateway, serve a canned model list, and spawn `infer agent`
children with `INFER_GATEWAY_MOCK=true` — the mock mode of the
[`inference-gateway/tokenless`](https://github.com/inference-gateway/tokenless)
library the CLI embeds. Each child serves itself a scenario gateway on an
ephemeral port — real infer binary, real tools, real approval flow, canned LLM
turns, zero tokens. Keep `list_models()`'s canned list (`openai/gpt-4o`,
`anthropic/claude-sonnet-4-5`, `openai/gpt-image-2`) in sync with the
`tokenless` gateway constants.

Scenarios are matched by regex against the first user message; this repo owns
its scenarios in `e2e/scenarios.yaml`, handed to the children via
`INFER_GATEWAY_MOCK_SCENARIOS` (the `task e2e` harness sets it automatically;
set it yourself for a manual `cargo tauri dev` session). Two more env
overrides matter here: `INFER_BIN=<path>` makes the desktop spawn a specific
infer build, and scenario tool paths resolve against the app's cwd. Prefer
mock mode for UI work; use a real model only when the change touches actual
agent behavior.

### Still impossible on macOS, plan around it

- **No console.** `console.error` output is unreachable — it is WKWebView, not
  Chrome, so the `claude-in-chrome` tools and CDP do not apply.
- **No DOM assertions, no `localStorage` reads.** WebKit keeps local storage in
  memory; `~/Library/WebKit/inference-gateway-desktop/WebsiteData/LocalStorage/`
  is empty on disk.

So verify the data path in the shell (run the same commands the backend runs, add
a Rust unit test for the parsing) and use the AX tree + screenshots for the UI.
Say plainly which parts stayed unverified.

`tauri-driver` + WebdriverIO is the official e2e route, but it is **Linux and
Windows only** — WKWebView exposes no WebDriver, so it cannot run on macOS. It
would run on the existing `ubuntu-24.04` CI runner.

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
