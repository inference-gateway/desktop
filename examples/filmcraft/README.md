# FilmCraft

One `editor` agent, a video editor of its own in every chat. Each chat with the agent gets its own Ubuntu desktop, and every one of them finds [FilmCraft](https://github.com/storytold/filmcraft) - an open-source video editor written in Rust - in a folder of this computer, so the package is downloaded once and never again.

The folder is shared with the app's local cluster under **Settings > Compute** and mounted read-only on the `ubuntu-desktop` workspace as a volume. A new computer does not have FilmCraft yet and the agent has no root access on it, so its instructions tell it to unpack the package from the mount into its home before it opens the editor. That takes about a second and needs no network.

Needs `docker`, `task` and the app's local cluster - the container button under **Settings > Compute**. The cluster runs operator v0.37.0, which mounts the volume, and the `infer` CLI has to be newer than v0.254.0 to pass it on.

```bash
task up     # FilmCraft's package in ~/filmcraft, the editor agent in ~/.infer/agents
```

`task up` downloads the package for this computer's architecture and checks it against the release's checksums. Then in the app:

1. Open **Settings > Compute**. Press **Add folder**, name it `filmcraft`, point it at `~/filmcraft` as a full path and switch it to read-only. Press **Save folders**.
2. Press **Recreate local cluster**, then **Delete its machines and recreate**. Docker mounts a folder only into a cluster it creates, so this deletes the machines of the local cluster.
3. Open **Settings > Workspaces** and edit `ubuntu-desktop`. Press **Add volume**, name it `filmcraft`, pick the folder as its source and mount it at `/mnt/filmcraft`. Press **Save workspace**.
4. Press **Restart CLI** so the daemon reads the volume.
5. Open the **Agents** filter and send `editor` its task: `open filmcraft`. It runs on the model selected in the top bar, which has to read screenshots - give it a model of its own in the agent editor to pin one.
6. Start a second chat with `editor` for a second editor. Open **Computers** in the top bar to watch them side by side.

The package sits one level below the mount path, under the folder's name: `/mnt/filmcraft/filmcraft`. FilmCraft renders on the CPU and says so in its status bar, on Docker because the computers have no GPU and on Podman because it does not pick the one they have - see [the GPU on the local cluster](../../docs/gpu.md).

```bash
task down   # delete the package and the agent
```

`task down` leaves the folder and the volume in Settings. Remove them there, and recreate the cluster once more to stop mounting the folder.

## A folder of your own

```bash
task up FOLDER=~/Movies/tools VERSION=0.19.0
```

`FOLDER` is where the package lands and `VERSION` is the FilmCraft release. Share footage the same way: a second read-only folder, mounted on the workspace, is raw material the agent copies before it edits.

The agent uses its computer without asking for approval - it is a pod of its own, not your machine.
