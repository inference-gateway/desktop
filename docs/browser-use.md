# Browser use (opentask extension)

Settings > General > **Browser Use** lets the agent drive your everyday browser through the [opentask](https://github.com/inference-gateway/opentask) extension - navigate, click, type, read, screenshot and list tabs - which is usually cheaper and more reliable than Computer Use.

Enabling it writes `~/.infer/browser_use.yaml` (`enabled: true`, `backend: extension`, and a generated `extension.token` if none is set) and restarts the `infer daemon` the app runs its chats through. The daemon binds `127.0.0.1:52789` (`binding.port` in `~/.infer/daemon.yaml`, or `extension.port`) for the extension, the desktop and any standalone `infer` session alike.

Paste the port and token shown in Settings into the extension options. The globe icon in the status bar (next to the auto-approve bolt) turns green once it connects, as the daemon reports the extension's state. Every chat is a thread of the daemon, so the agent's `browser_*` tools reach the extension through it, and the extension's side panel shows the same threads.

## On an agent's computer

An agent with a [computer of its own](agents.md#a-computer-of-their-own) browses on that computer instead. The workspace **Provision local cluster** creates starts with **User data** that installs Firefox ESR with opentask whenever a computer boots. The script generates a token and hands it to the extension through Firefox's enterprise policy (`3rdparty.Extensions`) and to the computer's daemon (`browser_use.yaml`, port 52790). Firefox does not start with the computer. Once an agent opens it, the extension connects on its own, with no options to fill in. Edit the script in Settings > Workspaces to install another browser or anything else the agents need.
