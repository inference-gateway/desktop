# Voiceover

You record a short screen walkthrough, send a handful of prompts, and export a narrated MP4 - on the app's own timeline, not on a computer of the agent's own and not in FilmCraft. The bundled `video-editing` skill writes `demo.timeline.json` and makes the audio. You review, edit and export on the timeline that sits beside the chat.

The example runs token-free in mock mode: `examples/voiceover/scenarios.yaml` scripts exactly the prompts below, modelled on the scenarios of [e2e/scenarios.yaml](../../e2e/scenarios.yaml).

## What you need

- `task` and this checkout, and a screen recording of about 40 seconds. Rename it `demo.mov` - **Add recording** copies into the project under the file's own name, and the scripted timeline points at `media/demo.mov`.
- A voice sample recorded in Settings > [Voice samples](../../docs/voice.md) under the default sample name `my-voice` - the scripted timeline pins the track's Voice to `my-voice.wav`, and you can pick another sample in the dropdown on the voice lane.
- Text to speech enabled and a speech model chosen there, plus a vision model under Settings > General. The Projects tab warns until both are set - see [Content projects](../../docs/content.md).

## Try it token-free

From the checkout:

```bash
DESKTOP_MOCK=true INFER_GATEWAY_MOCK_SCENARIOS=examples/voiceover/scenarios.yaml task dev
```

The app takes its model list from that file's `models:` block, the `infer` children answer their turns from it, and a prompt that matches no scenario gets the fallback answer. Mock mode skips installing the Content tools, so **Export** needs an ffmpeg with H.264 encoding on your PATH (a previous real Content project would also have installed one). For a real run, start the app normally, with your provider key under Settings > API Keys, and everything below behaves the same except the agent actually reads frames, transcribes and speaks.

## Set up

1. Create a project named `demo` (New project in the sidebar) and pick **Content** in `Type of project demo` under Settings > Projects. In a real run this installs the bundled skill, ffmpeg and whisper into `~/.infer`. In mock mode it just clears the warning.
2. In the demo project, press **Add recording** in the media pool at the bottom of the timeline and pick your renamed `demo.mov`. It copies into the project's `media/` folder.
3. The timeline is empty until the first prompt lands. Approve each Write card as it appears, or move the status bar's mode to Auto to run without asking.

## Run it

1. **Add voice.** Send:

   ```text
   Add my cloned voice to demo.mov: write demo.timeline.json and make the audio for every clip.
   ```

   The skill probes the recording, looks at keyframes with the vision model (and transcribes what you said, if the recording has speech), writes the timeline in `demo.timeline.json`, then speaks every clip with `TextToSpeech` in your voice sample and stops there - it never exports. Once the timeline exists the toolbar's sparkles button sends the same ask itself: **Add voice** while clips are untouched, **Redo drafts** afterwards.

   Leaves: `demo.mov` on the video track over 0-42.3 and a voice lane with three draft clips, amber until they have audio, with the lane's Voice dropdown on `my-voice.wav`. In mock mode nothing is synthesized, so the clips stay drafts.

2. **Edit a line and redo drafts.** Select the middle clip on the voice lane and rewrite its text in the inspector - the scripted wording is "Then we press Add captions for every spoken line." - then press **Redo drafts**. The skill re-speaks only the draft clips, in the voice named on each clip, and never regenerates a done clip you did not edit. In mock mode the run reads the timeline and answers without changing it. The drafts stay drafts.

3. **Add captions.** Press the **Add captions** button (the letter T). It asks for a captions track following the captions rules of the skill and nothing else.

   Leaves: a captions lane holding one clip per spoken clip, text taken from the script and drawn bottom-centre in the classic style, which the lane's style menu can change.

4. **Ask for a title card.** Send:

   ```text
   Add a title card at the start.
   ```

   The skill writes the composition `cards/o1-title.html`, renders it with HyperFrames to `media/o1-title.mov` and places it on an overlay track over 0-4. In mock mode only the composition file and the timeline slot exist - the mock cannot render a card - so the overlay clip shows as missing until a real run fills it.

5. **Export.** Press **Export** in the toolbar. The timeline renders frame by frame through ffmpeg into the project's `export/` folder, with a progress counter and a **Cancel** button, and the app reveals the file when it is done. In mock mode the draft clips have no audio yet, so the export is the recording, captions and the card, silent.

6. **Change your mind.** Every saved edit makes a checkpoint, yours and the agent's alike: **Undo** (`Ctrl/Cmd+Z`) steps back through them, for example taking the title card back out, **Redo** (`Ctrl/Cmd+Shift+Z`) steps forward again, and **Split** (`S` or `Ctrl+B`) cuts the clips under the playhead in two.

## What the mock does not do

- No audio: mock mode never runs `TextToSpeech`, so voice clips keep their amber drafts and the export stays silent.
- No card render: the title card is the HTML composition and a timeline slot only. HyperFrames runs in a real session.
- Scripted rewrites: the captions and title-card turns rewrite `demo.timeline.json` with their own wording, so an edit that differs from the suggested line is put back. A real model reads the file first and keeps whatever you wrote.
