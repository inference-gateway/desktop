# Browser use (opentask extension)

Settings > General > **Browser Use** lets the agent drive your everyday browser through the [opentask](https://github.com/inference-gateway/opentask) extension - navigate, click, type, read, screenshot and list tabs - which is usually cheaper and more reliable than Computer Use.

Enabling it writes `~/.infer/browser_use.yaml` (`enabled: true`, `backend: extension`, and a generated `extension.token` if none is set) and restarts the `infer daemon` the app runs its chats through. The daemon binds `127.0.0.1:52789` (`binding.port` in `~/.infer/daemon.yaml`, or `extension.port`) for the extension, the desktop and any standalone `infer` session alike.

Paste the port and token shown in Settings into the extension options. The globe icon in the status bar (next to the auto-approve bolt) turns green once it connects, as the daemon reports the extension's state. Every chat is a thread of the daemon, so the agent's `browser_*` tools reach the extension through it, and the extension's side panel shows the same threads.
