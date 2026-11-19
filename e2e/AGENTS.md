# AGENTS.md - e2e suite

`task e2e` runs the YAML tests in `e2e/tests/` against a fresh mock-mode build, driving the real UI through the macOS accessibility tree - no WebDriver, no `tauri-driver`. Every test file demos exactly one feature and reads top to bottom as a walkthrough: `name:` states the behaviour, `narration:` carries one line per step that the runner prints as it goes, and `steps:` drives the UI. The element ids and accessible names the steps use are the DOM contract in `frontend/AGENTS.md`.

Running the tests (macOS only):

```bash
task e2e -- tests/approval-approve.yaml   # one feature
task e2e                                  # all features, alphabetical
cargo test -p desktop-e2e                 # parse/validate the YAML only (what the CI workflow does)
```

Every model turn is tokenless: the runner launches the app with `DESKTOP_MOCK=true` and `INFER_GATEWAY_MOCK_SCENARIOS=e2e/scenarios.yaml`, and the spawned `infer` children serve those scripted scenarios - nothing hits a real provider. A prompt with no matching scenario gets the `fallback` answer, so a test prompt must only promise what its scenario's turns produce. The `models:` block at the top of `scenarios.yaml` is the single source of truth for the mock-mode model list (the app reads it instead of hitting a gateway), so manual mock mode needs the same env var for both turns and models:

```bash
DESKTOP_MOCK=true INFER_GATEWAY_MOCK_SCENARIOS=e2e/scenarios.yaml task dev
```

Test-file extras: `record: true` wraps the run in `screencapture -v` and writes `artifacts/<slug>.mov` (no editing, no captions - that is what the app's own timeline tooling is for). The runner's `--record` flag (`task e2e:record`) forces recording on every test, ignoring the per-file flag, so all e2e tests run while recording. A `drag` step moves the real mouse (CoreGraphics events) from one element onto another, each found by its accessible name or description, so the run takes over the cursor. `from_dx` shifts the press onto a handle that has no name, and `assert_order` checks that one element sits above another. Missing `narration:` entries fall back to the mechanical step label, and `cargo test -p desktop-e2e` rejects duplicate test names, narration lists longer than the step list, and semicolons in test names and narration. The remaining `DESKTOP_MOCK` fakes (permissions, stt, timeline, screen records, scheduler, updates) are OS/tool shims, not LLM shims. The updates fake (`backend/src/updates_mock.rs`) reports a CLI one release behind until it is applied, so mock mode always starts with **Update** in the top bar, and a forced CLI install downloads nothing. `voice-input.yaml` records through the real microphone, so the terminal running the suite needs Microphone access as well as Accessibility.

## Running in a container

`task e2e -- --podman` runs the same tests in a Podman container, against the Linux build under Xvfb, so a run never touches this machine's screen, mouse or app. It needs Podman with a running machine (`podman machine start`).

```bash
task e2e -- --podman tests/approval-approve.yaml   # one feature, in the container
task e2e -- --podman                               # all features, in the container
```

The runner builds the image from `e2e/Containerfile`, mounts the repo at `/src` and runs itself in the container. The Linux build and cargo's downloads live in the `desktop-e2e-target` and `desktop-e2e-cargo-registry` volumes, so the host's `target/` and `node_modules/` stay untouched and only `dist/` comes from the host, which `task e2e` builds first. Inside, `src/webdriver.rs` drives the app over WebDriver through `tauri-driver` instead of the accessibility tree. It finds elements by the names the macOS run reads - the `aria-label`, else the rendered text without its `aria-hidden` parts - so the YAML runs unchanged. A failing test leaves its log and a screenshot of the page in `e2e/artifacts/`, printed under `/src/e2e/artifacts/`. Nothing is recorded there, whatever `record:` or `--record` say.

These tests still need the macOS run:

- `computer-use.yaml` checks the overlay against the macOS menu bar and Dock.
- `voice-input.yaml` records through a real microphone.
- `artifacts-draft-review.yaml` makes its draft with the macOS `say` command.
- `transcript-overflow.yaml` streams its long answer slower than its 30 s wait under the container's software rendering.
- `cluster-folders.yaml` clicks **Add folder** before the saved folders load, and the load replaces the new row.

## Demo recordings

Demo recordings (`@infer /demo` or `@opentask /demo`, Linux CI): the `demo` skill records the Xvfb display `:99` (1280x720, no window manager). Run the build in the `demo` tmux session - a cold build takes about 2.5 minutes, longer than the 120 s Bash timeout - and use the binary, since `cargo-tauri` is not installed there. The app needs 20-30 s to show its window on the runner (GTK waits for an accessibility bus that is not there) and longer while recording, so rehearse and start the take only once the window is up. The 760px-tall main window overflows the display: wait for it to be visible, resize it, and repeat the resize once the UI has rendered, because `center: true` can move it back. Send only prompts that a `scenarios.yaml` scenario answers:

```bash
bun run build
cargo build -p inference-gateway-desktop
DISPLAY=:99 WEBKIT_DISABLE_DMABUF_RENDERER=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 LIBGL_ALWAYS_SOFTWARE=1 DESKTOP_MOCK=true INFER_GATEWAY_MOCK_SCENARIOS=$PWD/e2e/scenarios.yaml ./target/debug/inference-gateway-desktop &
DISPLAY=:99 xdotool search --onlyvisible --sync --name '^Inference Gateway Desktop$' windowmove 0 0 windowsize 1280 720
```
