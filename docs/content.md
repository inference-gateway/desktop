# Content projects

The app's own video editor. A project you switch to the **Content** type stops being a code workspace and becomes a talking-clip editor: you add media to a pool, the bundled `video-editing` skill writes a timeline and speaks over it, and you arrange, cut, caption and export the result on the timeline in the same window as the chat - no FilmCraft, no computer of the agent's own.

## Making one

Open **Settings > Projects**, find the project card and pick **Content** in the `Type of project <name>` select (a project starts as **Code**). The tab warns while either prerequisite is missing:

- **Text to speech** enabled - live under Settings > [Voice samples](voice.md).
- **A vision model** under Settings > General - the agent uses it to describe what each frame of your recording shows (the CLI's `ImageDecode` tool).

Saving the switch installs what the skill needs on this computer, into `~/.infer`: the bundled `video-editing` skill, `ffmpeg` and `whisper-cli` in `~/.infer/bin/tools`, and the tiny whisper transcription model. Nothing is downloaded again on the next project.

## The timeline

The editor renders a `<stem>.timeline.json` file in the project directory - `demo.mov` gets `demo.timeline.json`, an agent's first timeline for an untitled project is `main.timeline.json`. One timeline per video; when the project holds several, a select next to the title switches between them. The timeline file is the contract: the agent reads it before every change and writes it back after each step, and your own edits save to the same file a moment after you let go.

- **Media pool** - every video, audio and image file in the project's `media/` folder. **Add recording** picks a file and copies it in; dropping files from Finder imports them too. Drag a pool file onto a lane to place it, or add a voice clip or caption straight at the playhead with the lane's + button.
- **Tracks** - a lane each for `video`, `audio`, `overlay` and `captions`. Spoken clips on an audio track hold their text and voice, drafts are amber until they have audio, overlays are cards, captions are burned-in text. An audio track's **Voice** dropdown and a captions lane's style menu live on the lane.
- **Preview** - the playhead scrubs, Space plays, and the selected video clip can be dragged and zoomed to frame it, with keyframes for moves. The playhead and play/pause (Space) sit above the lanes.

## The timeline's controls

- **Split** - the scissors, or `S` / `Ctrl+B`, cuts every clip under the playhead in two.
- **Undo** / **Redo** - the timeline keeps a checkpoint for every saved edit, yours and the agent's alike; `Ctrl/Cmd+Z` steps back and `Ctrl/Cmd+Shift+Z` forward.
- **Add voice** / **Redo drafts** - the sparkles asks the agent to voice the video (or re-speak the clips you edited), see [Voice](voice.md).
- **Add captions** - asks the agent to write a captions track from the spoken clips or the recording's transcript.
- **Export** - renders the frame size set on the toolbar (`1920x1080`, `1080x1920` for vertical, `1350x1350` for square) with ffmpeg to `export/<output>` in the project, with progress and a **Cancel** button. It needs a full ffmpeg with H.264 encoding; the bundled one has it.

Overlay cards are HTML compositions rendered with HyperFrames; [Avatars](avatars.md) covers talking-avatar clips, and [examples/voiceover](../examples/voiceover) walks a recording through to an exported MP4, token-free in mock mode.
