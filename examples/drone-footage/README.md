# Drone footage

One `drone-editor` agent turns raw drone footage into a finished clip on a computer of its own, and you only download the result.

Ask it for "a 40 second clip" and it reads the footage from a read-only folder, finds the stretches with movement, stabilises them with [Gyroflow](https://github.com/gyroflow/gyroflow), builds and grades the edit in [FilmCraft](https://github.com/storytold/filmcraft) starting from DJI's own LUT, generates sound effects and then music with ElevenLabs, and exports the edit once per platform shape. The exports show up under the chat's **Artifacts** menu, where **Save artifact** puts one in Downloads.

It builds on [examples/filmcraft](../filmcraft): the same idea of tools in a folder shared with the local cluster, plus the three things that example leaves out - content the agent works on, a secret, and a deliverable.

| Shape | Size | File | For |
| --- | --- | --- | --- |
| Landscape 16:9 | 1920x1080 | `landscape.mp4` | YouTube, the web |
| Vertical 9:16 | 1080x1920 | `vertical.mp4` | TikTok, Instagram Reels, YouTube Shorts |
| Square 1:1 | 1080x1080 | `square.mp4` | Instagram and Facebook feeds |
| Portrait 4:5 | 1080x1350 | `portrait.mp4` | Instagram feed |

## What you need

- `docker`, `task` and the app's local cluster - the container button under **Settings > Compute**.
- `infer` CLI v0.255.0 and operator v0.36.0 or newer, which mount the folders.
- A GPU on the agent's computer. Gyroflow is unusable without one - see [The GPU](#the-gpu). The app cannot give its local cluster one yet ([#536](https://github.com/inference-gateway/desktop/issues/536)), so today this example runs only with the stopgap that issue describes, on an Apple silicon Mac.
- An ElevenLabs API key. The sound effects and the music are real API calls and cost credits, so this example cannot run on the mock model.
- A model that follows long instructions and reads pictures, since the agent looks at frames to judge its picks, its grade and its crops.
- Your own footage, with gyro data. The example was run on one clip from a DJI Avata 2: 3840x2880, 29.97 fps, 10-bit HEVC in D-Log M, with electronic stabilisation off, which is what leaves the gyro data in the file. Gyroflow reads the data of many other cameras. The LUT is DJI's for the Avata 2 - for another camera, put its `.cube` in the tools folder instead.

## Set up

```bash
task up     # FilmCraft, Gyroflow and DJI's LUT in ~/drone-tools, the drone-editor agent in ~/.infer/agents
```

`task up` downloads FilmCraft's package for this computer's architecture and checks it against the release's checksums, and downloads DJI's LUT and checks it against the checksum of the file the example was run with. Gyroflow publishes no Linux build for arm64, so on an Apple silicon Mac `task up` builds v1.7.0 from source in Docker, which takes about ten minutes the first time.

Put your footage in a folder of its own, for example `~/footage`. Then in the app:

1. Open **Settings > Compute**. Press **Add folder**, name it `tools`, point it at `~/drone-tools` as a full path and switch it to read-only. Add a second folder named `footage` for your footage folder, read-only too. Press **Save folders**.
2. Press **Provision local cluster**. If the cluster already runs, press **Recreate local cluster**, then **Delete its machines and recreate** - Docker mounts a folder only into a cluster it creates, so this deletes the machines of the local cluster.
3. Open **Settings > Workspaces** and edit `ubuntu-desktop`. Press **Add volume**, name it `tools`, pick `~/drone-tools` as its source and mount it at `/mnt/tools`. Add a second volume named `footage`, pick your footage folder and mount it at `/mnt/footage`. Press **Save workspace**.
4. Open **Settings > Secrets** and press **New secret**. Label it `elevenlabs`, name its variable `ELEVENLABS_KEY`, paste your key and tick **Workspace ubuntu-desktop**. Press **Save secret**. The app refuses the name `ELEVENLABS_API_KEY` here, because that one is a provider key of the gateway and belongs under **API Keys**.
5. Press **Restart CLI** so the daemon reads the volumes and the secret.

The key is kept in `~/.infer/auth.yaml` and reaches the computer as a variable. It appears in no file of this example and the agent is told never to print it.

## Run it

Open the **Agents** filter, select `drone-editor` so the composer reads **Message drone-editor...**, pick a model in the top bar and send:

```text
Make a 40 second clip from the footage in all four shapes.
```

Open **Computers** in the top bar to watch. The agent opens FilmCraft on its computer and sends every edit to that window, so the timeline, the grade and the exports appear there as it works. When it reports, open **Artifacts** and press **Save artifact** on the exports you want.

Ask for less, or for something else, in the same sentence:

```text
Make a 20 second clip, vertical only.
Make a 30 second clip in landscape and in 2560x1080 for an ultrawide screen.
```

The agent stops and says why when it cannot do the job as described - footage without gyro data, a computer without a GPU, a missing key.

### The footage stays as it is

The footage folder is read-only twice: under **Settings > Compute**, so Docker mounts it read-only into the cluster, and on the workspace's volume. A write from the computer is refused:

```console
$ touch /mnt/footage/footage/test
touch: cannot touch '/mnt/footage/footage/test': Read-only file system
```

The agent analyses the footage in place and Gyroflow reads it from the mount. Everything it writes goes into its own working directory.

## The GPU

Gyroflow warps every frame, and that is GPU work. On the clip above, in a Linux VM with 8 CPUs:

| | Without a GPU | With the Mac's GPU |
| --- | --- | --- |
| Stabilise 5 s of 4K (150 frames) | 18 frames after 10 minutes | about 7 s |

FilmCraft is different. Its export is no faster with the GPU - 20 frames a second on software rendering, 17 to 18 with the GPU allowed - because decoding and encoding video are most of the work, and no Linux VM on a Mac reaches Apple's hardware codecs. The agent's instructions keep FilmCraft on software rendering and leave the GPU to Gyroflow.

The app's local cluster runs in Docker, whose VM has no GPU, so a computer of the local cluster has none either. [#536](https://github.com/inference-gateway/desktop/issues/536) is about giving it one, and describes the stopgap this example was run with: the cluster on Podman with libkrun, which hands the Mac's GPU to the computers.

## The run this was written from

One run, on an M4 Pro with macOS 27 and the stopgap of #536, with `ollama_cloud/glm-5.3-flash`:

| Step | Time |
| --- | --- |
| Find the action and pick the segments | about 3 minutes |
| Stabilise 40 s with Gyroflow | 3 min 50 s, plus 84 s for ranges the agent changed its mind on |
| Sound effects, music, edit and grade | about 6 minutes |
| Four exports | about 2.5 minutes each |
| Whole run | 25 minutes |

It took 142 model requests and 183 tool calls, with 18.6 million input tokens, 16.6 million of them cached, and 106 thousand output tokens. The CLI knows no price for that model, so no cost in money was recorded. ElevenLabs was called seven times - six sound effects and one 40 second music track.

Each export came out at 40.04 s and about 40 MB. The agent exports at 8 Mbps on purpose: the app downloads an artifact in one piece of at most 64 MiB.

The computers of a workspace have no CPU or memory limit of their own yet. With a 12 GB VM, a first attempt ran five encodes at once and the computer was killed for memory. The run above had 24 GB, FilmCraft alone held about 10 GB while exporting, and the agent's instructions now allow one heavy job at a time.

## Clean up

```bash
task down   # delete the tools and the agent
```

`task down` never touches your footage. It leaves the folders, the volumes and the secret in Settings - remove them there, and recreate the cluster once more to stop mounting the folders. The exports you saved stay in Downloads.

The agent uses its computer without asking for approval - it is a pod of its own, not your machine.
