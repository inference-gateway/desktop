# Feature video

One `feature-editor` agent turns a raw screen recording into a tutorial on a computer of its own, and you only approve and download.

Give it a recording of you using a feature and it cuts the stretches where nothing happens, speeds up the slow parts, zooms in where the action is, writes a narration and speaks it in your voice, adds captions and title, chapter and outro cards, and introduces you with a name tag when you ask. It edits in [FilmCraft](https://github.com/storytold/filmcraft), which also makes the cards, and speaks with the `infer` CLI's text to speech. The result shows up under the chat's **Artifacts** menu.

It builds on [examples/drone-footage](../drone-footage): the same read-only footage folder and the same draft you approve before the final export. It needs no GPU, no API key and no tools folder.

| File | What it is |
| --- | --- |
| `tutorial.mp4` | The tutorial, 1920x1080, captions in the picture |
| `tutorial.srt` | The captions as a file, for a player or YouTube |
| `chapters.txt` | The chapters with their times, for the video's description |

## What you need

- `task` and the app's local cluster - the container button under **Settings > Compute**.
- `infer` CLI newer than v0.258.0 and operator v0.37.0 or newer, which give the workspace's gateway its speech engine and bring your voice samples to the computer.
- A model that follows long instructions and reads pictures. The recording has no sound, so the agent writes the narration from the frames it looks at.
- A screen recording in H.264, 8-bit. A macOS screen recording is one.
- For your own voice, a sample under **Settings > Voice samples**: 10 to 30 seconds of you speaking, one speaker, no music, no noise. Your samples travel to the agent's computer with the chat. Without one the agent uses a stock voice.

## Set up

```bash
task up     # the feature-editor agent in ~/.infer/agents
```

Put the recording in a folder of its own, for example `~/footage`. Then in the app:

1. Open **Settings > Compute**. Press **Add folder**, name it `footage`, point it at your folder as a full path and switch it to read-only. Press **Save folders**.
2. Press **Provision local cluster**. If the cluster already runs, press **Recreate local cluster**, then **Delete its machines and recreate** - a folder is mounted only into a cluster created after it was added, so this deletes the machines of the local cluster.
3. Open **Settings > Workspaces** and edit `ubuntu-desktop`. Press **Add volume**, name it `footage`, pick the `footage` folder as its source and mount it at `/mnt/footage`.
4. In the same form, check that **Text to speech** is on. It gives the agent the `TextToSpeech` tool and sends your voice samples along with each chat. Press **Save workspace**.
5. Press **Restart CLI** so the daemon reads the volume and the switch.

## Run it

Open the **Agents** filter, select `feature-editor` so the composer reads **Message feature-editor...**, pick a model in the top bar and send:

```text
Edit the recording into a tutorial. It shows how to add a workspace volume.
```

Say what the feature is and who the tutorial is for - the agent sees the screen, not your intent. Name the voice sample to speak in, such as `in my Confident voice`. Without a name the agent uses your only sample, or asks which one when you have several.

Open **Computers** in the top bar to watch. FilmCraft opens first and the whole job shows in its window: the playhead moves to what the agent looks at, each beat of the plan is a coloured marker on the recording, and the tutorial is built row by row. The agent stops twice and asks you a question:

1. **The edit plan.** A table of beats - the steps it keeps - with what happens to each (kept, sped up, zoomed), the sentence it will say over it, where the cards go, and the voice it will speak in. Nothing is built yet, so this is the cheap place to change things:

   ```text
   Drop beat 4, and say "workspace" instead of "environment" everywhere.
   ```

2. **The draft.** **Artifacts** in the status bar lights up when `draft-v1.mp4` arrives - open it and press **Preview**. Approve it, or say what to change:

   ```text
   The zoom on the terminal is too tight.
   Speed up the build even more.
   ```

Each round leaves a new draft, `draft-v2.mp4` and so on. Once you approve, the agent exports the final and reports. Open **Artifacts** and press **Save artifact** on the files you want.

Ask for something else in the same sentence:

```text
Edit the recording into a 90 second tutorial, no captions in the picture.
```

### The recording stays as it is

The footage folder is read-only twice: under **Settings > Compute** and on the workspace's volume. The agent analyses the recording in place, and everything it writes goes into its own working directory.

## What the agent downloads

A new computer has no editor and the agent has no root access, so it fetches FilmCraft, the latest package, into its working directory the first time and checks it against its checksum. It survives a restart of the computer. Without the editor the agent stops and reports - it never edits around it.

The voice model is not on the computer. The workspace's gateway runs it: it downloads about 1.4 GB each time it starts, which it does again after every stretch with no chat running, and it holds about 6 GiB of memory while it speaks a sentence. A sentence took 20 seconds when measured on the gateway's 2 CPUs.

## Not run end to end yet

One full run on the local cluster built and exported a tutorial from an 11 minute recording, speaking in a sample that came along, with an earlier version of these instructions. The current ones have not run end to end yet. Their FilmCraft commands - the markers, the cards, the name tag and the export's loudness - were each checked against FilmCraft 0.20.1 without its window.

Expect the first run to need a correction, and report what the agent stopped on.

## Clean up

```bash
task down   # take the agent back out of ~/.infer
```

`task down` never touches your footage. It leaves the folder and the volume in Settings - remove them there, and recreate the cluster once more to stop mounting the folder. The files you saved stay in Downloads.

The agent uses its computer without asking for approval - it is a pod of its own, not your machine.
