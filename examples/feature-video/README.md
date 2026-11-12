# Feature video

One `feature-editor` agent turns a raw screen recording into a tutorial on a computer of its own, and you only approve and download.

Give it a recording of you using a feature and it cuts the stretches where nothing happens, speeds up the slow parts, zooms in where the action is, writes a narration and speaks it in your voice, adds captions, and renders title, chapter and outro cards. It edits in [FilmCraft](https://github.com/storytold/filmcraft), speaks with the `infer` CLI's text to speech and renders the cards with [HyperFrames](https://github.com/heygen-com/hyperframes). The result shows up under the chat's **Artifacts** menu.

It builds on [examples/drone-footage](../drone-footage): the same read-only footage folder and the same draft you approve before the final export. It needs no GPU, no API key and no tools folder.

| File | What it is |
| --- | --- |
| `tutorial.mp4` | The tutorial, 1920x1080, captions in the picture |
| `tutorial.srt` | The captions as a file, for a player or YouTube |
| `chapters.txt` | The chapters with their times, for the video's description |

## What you need

- `task` and the app's local cluster - the container button under **Settings > Compute**.
- `infer` CLI v0.258.0 or newer.
- A model that follows long instructions and reads pictures. The recording has no sound, so the agent writes the narration from the frames it looks at.
- A screen recording in H.264, 8-bit. A macOS screen recording is one.
- For your own voice, a sample under **Settings > Voice samples**: 10 to 30 seconds of you speaking, one speaker, no music, no noise. The agent reads the samples you already have, and one you record later shows up on its computer as soon as it is saved. Without one the agent uses a stock voice.

## Set up

```bash
task up     # the feature-editor agent in ~/.infer/agents
```

Put the recording in a folder of its own, for example `~/footage`. Then in the app:

1. Open **Settings > Compute**. Press **Add folder**, name it `footage`, point it at your folder as a full path and switch it to read-only. Add a second folder named `voices` for `~/.infer/models/tts/samples`, where the app keeps your voice samples - as a full path, `/Users/<you>/.infer/models/tts/samples`, and read-only too. Press **Save folders**.
2. Press **Provision local cluster**. If the cluster already runs, press **Recreate local cluster**, then **Delete its machines and recreate** - a folder is mounted only into a cluster created after it was added, so this deletes the machines of the local cluster.
3. Open **Settings > Workspaces** and edit `ubuntu-desktop`. Press **Add volume**, name it `footage`, pick the `footage` folder as its source and mount it at `/mnt/footage`. Add a second volume named `voices`, pick the `voices` folder and mount it at `/mnt/voices`.
4. In the same form, add these lines to **Environment**. They give the agent the `TextToSpeech` tool, with the voice model running on its own computer:

   ```text
   INFER_TEXT_TO_SPEECH_ENABLED=true
   INFER_TEXT_TO_SPEECH_ENGINE=qwen3-tts
   INFER_TEXT_TO_SPEECH_REQUIRE_APPROVAL=false
   INFER_TEXT_TO_SPEECH_TIMEOUT=900
   ```

   Press **Save workspace**.
5. Press **Restart CLI** so the daemon reads the volume and the environment.

Provisioning the local cluster again can reset the workspace's environment. If the agent reports that it has no `TextToSpeech` tool, add the four lines again.

## Run it

Open the **Agents** filter, select `feature-editor` so the composer reads **Message feature-editor...**, pick a model in the top bar and send:

```text
Edit the recording into a tutorial. It shows how to add a workspace volume.
```

Say what the feature is and who the tutorial is for - the agent sees the screen, not your intent. Name the voice sample to speak in, such as `in my Confident voice`. Without a name the agent uses your only sample, or asks which one when you have several.

Open **Computers** in the top bar to watch. The agent stops twice for you:

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

### The recording and the samples stay as they are

Both folders are read-only twice: under **Settings > Compute** and on the workspace's volumes. The agent analyses the recording in place, copies the one voice sample it speaks in, and writes everything else into its own working directory.

## What the agent downloads

A new computer has none of its tools and the agent has no root access, so it fetches them into its working directory the first time, each checked against its checksum:

| What | For |
| --- | --- |
| FilmCraft, the latest package | The edit |
| Node.js v22 and a headless Chromium | HyperFrames, which renders a card from HTML |
| The voice model, about 1.4 GB | Text to speech |

The first two survive a restart of the computer. The voice model lives in the computer's home and is downloaded again on a new computer.

If the browser cannot start on the computer, the agent makes the cards as text in FilmCraft instead and says so.

## Not run end to end yet

The agent's instructions were written from FilmCraft's, HyperFrames' and the CLI's source and documentation. These parts have not run on a computer of the local cluster yet:

- FilmCraft on a variable frame rate screen recording of this size.
- The voice model on an arm64 computer without a GPU, and how long a sentence takes.
- HyperFrames' browser on the computer image, which may lack a library it needs.

Expect the first run to need a correction, and report what the agent stopped on.

## Clean up

```bash
task down   # take the agent back out of ~/.infer
```

`task down` never touches your footage. It leaves the folders, the volumes and the environment in Settings - remove them there, and recreate the cluster once more to stop mounting the folder. The files you saved stay in Downloads.

The agent uses its computer without asking for approval - it is a pod of its own, not your machine.
