---
name: video-editing
description: Add the user's own cloned voice to a screen recording - probe the video and pull scene keyframes with ffmpeg, describe them with ImageDecode (or write down what the user says in the recording with whisper-cli and clean it up), write a <stem>.timeline.json plan, and make the audio for each clip with TextToSpeech (voice_sample cloning) - and place animated cards (title, lower third, callout, step, chart) rendered with HyperFrames on the timeline's overlay track - and lip-synced talking clips of the user's avatar rendered with TextToVideo. The desktop renders the export; the agent never muxes. Use when the user asks to add a voiceover, add their voice, explain a video, redo the voice on a recording, redo clips of an existing timeline, cut, split or trim video or audio clips on the timeline, add captions or subtitles to a video, add a card, title or overlay to a video, or add a talking avatar, presenter or talking head.
license: Apache-2.0
---

# Video Editing

Use this skill when the user gives you a video (usually a macOS screen recording in the working
directory, silent or with the user talking over it) and wants their voice added, asks to redo clips of
an existing `<stem>.timeline.json`, wants a card (title, lower third, callout, step counter, chart)
layered over the video (see Overlay cards), or wants a talking avatar of themselves (see Avatars).

The desktop app renders `<stem>.timeline.json` as an editable timeline, so the file is the contract:
always read it first if it exists, always write it back after every step that changes it.

## Tools you may use

Only these, nothing else: `Bash` for `ffmpeg`, `whisper-cli`, `cp`, `mkdir`, `ls`, `grep` and `rm` (scratch
files only) inside the working directory, plus `node --version` and `npx hyperframes render` for cards;
`Read` and `Write` for the timeline JSON and card HTML; `ImageDecode`; `TextToSpeech`; `TextToVideo`
for avatar clips; `AskUserQuestion` to pick the voice sample and the avatar. Do not
use `WebFetch`, `WebSearch`, `find`, package managers, or any other binary, and do not look for
this skill, other skills or tools on disk: everything you need is listed below at fixed paths.

## Prerequisites

Everything below is installed by the desktop when a project is switched to the Content type. Do
not download, build, symlink or search the disk for tools, and do not create directories anywhere
under `~/.infer`; the only place you write is the working directory.

- `ffmpeg` on `PATH` (the desktop's own copy lives in `~/.infer/bin/tools/ffmpeg`). There is no `ffprobe`;
  probe with `ffmpeg -hide_banner -i <file> 2>&1 | grep -E 'Duration|Stream'` (bare `ffmpeg -i` always
  exits 1 for lack of an output file; the `grep` exits 0 when the file was read).
- `~/.infer/bin/tools/whisper-cli` with the model `~/.infer/models/whisper/ggml-tiny.bin` (for
  `source_audio: transcribe` and for caption word timing). Use a larger model only if one already
  exists in that directory.
- An ffmpeg that can mix audio and encode H.264: `ffmpeg -hide_banner -encoders | grep -c ' libx264 '`
  prints `1`. Without it the desktop's Export fails, and the user needs a full ffmpeg
  (`brew install ffmpeg`). Captions no longer need libass - the desktop draws them itself.
- The `ImageDecode` tool (`vision.annotator.enabled` with a vision model, typically
  `ollama/qwen3-vl:2b` for a local setup) and the `TextToSpeech` tool (`text_to_speech.enabled`).
- A voice sample: a 10-30 s `.wav` of the user speaking, kept by the desktop in
  `~/.infer/models/tts/samples/`. `TextToSpeech` takes a bare file name and looks it up in the
  working directory, then in that library, so pass the sample's name as is (e.g. `eden.wav`) and
  never copy it into the project. See Picking the voice for which one.
- For avatar clips only: the `TextToVideo` tool (`text_to_video.enabled`) and an avatar, a folder of
  portraits of one person in `~/.infer/avatars/<name>/`. `TextToVideo` takes the folder's bare name
  (e.g. `presenter`); never copy the portraits into the project.

If any of these is missing, stop and tell the user exactly which one: tools and the model are
installed by switching the project to Content in Settings > Projects; the agent tools are
enabled in Settings > General; voice samples are recorded in Settings > Voice samples; avatars are
created in Settings > Avatars.

Media lives in `media/` inside the working directory: the recordings and music the user added
(the desktop shows this folder as the media pool) and every voice clip you synthesize. Timeline
`src` paths point there (`media/<file>`); a bare `src` at the root is only for old projects.
Scratch files (`frames/`, `audio.wav`, `transcript.json`, word timings) live in `tmp/` (`mkdir -p tmp`),
which the desktop keeps out of git and the media pool. The one exception is `voice.wav` cut from the
recording: `TextToSpeech` only takes a bare name, so it stays at the root.

## Picking the voice

Never choose the voice silently: it is the one thing only the user can judge.

1. A clip's own `voice_sample`, and the track's for clips without one, is the user's pick from the
   desktop's dropdowns. Use exactly that one, keep the field as written, and do not ask - a redo
   prompt that names a sample is that pick, not an invitation to ask again. Only when the user says
   they want a different voice do you go to step 2.
2. Otherwise list the library, `ls ~/.infer/models/tts/samples/`, and ask with `AskUserQuestion`:
   one option per sample, plus `Random`, plus `My voice from the recording` when `source_audio` is
   `transcribe`. Put the track's current `voice_sample` first when it has one. Ask once per run,
   never per clip, and only for the questions above - never ask about the script, the timing or the
   clips.
3. Nothing to choose between (one sample and no recording to cut from) - use it without asking. No
   samples at all and no recording speech: stop and say so, samples are recorded in
   Settings > Voice samples.
4. On `Random`, pick one yourself and say which. On a library sample, write its bare name into
   the track's `voice_sample` so the desktop shows the voice that was used. On the recording, set
   `voice_sample` to `"recording"` and cut the sample from it (see Source audio).
5. Write the same name into each clip you synthesize (see the contract's clip `voice_sample`), so a
   clip keeps the voice it was made with when a later run picks a different one.

## Timeline contract (`<stem>.timeline.json`)

```json
{
  "version": 1,
  "duration": 42.3,
  "output": "demo.with-voice.mp4",
  "resolution": "1920x1080",
  "fps": 30,
  "source_audio": "transcribe",
  "tracks": [
    {
      "id": "video",
      "kind": "video",
      "clips": [{ "id": "v1", "src": "media/demo.mov", "start": 0, "end": 42.3 }]
    },
    {
      "id": "voice",
      "kind": "audio",
      "voice_sample": "eden.wav",
      "clips": [
        {
          "id": "s1",
          "start": 0.0,
          "end": 6.2,
          "text": "First we open the settings panel.",
          "src": "media/demo-s1.wav",
          "voice_sample": "eden.wav",
          "status": "done"
        },
        { "id": "s2", "start": 6.2, "end": 12.0, "text": "", "status": "draft" }
      ]
    },
    { "id": "music", "kind": "audio", "gain": 0.2, "clips": [] },
    {
      "id": "captions",
      "kind": "captions",
      "style": "classic",
      "position": "bottom",
      "clips": [
        { "id": "c1", "start": 0.0, "end": 4.6, "text": "First we open the settings panel." },
        {
          "id": "c2",
          "start": 4.6,
          "end": 9.2,
          "text": "Then pick a voice",
          "words": [
            { "text": "Then", "start": 4.6, "end": 5.1 },
            { "text": "pick", "start": 5.1, "end": 5.5 },
            { "text": "a", "start": 5.5, "end": 5.6 },
            { "text": "voice", "start": 5.9, "end": 6.4 }
          ]
        }
      ]
    },
    {
      "id": "cards",
      "kind": "overlay",
      "clips": [
        { "id": "o1", "src": "media/o1-title.mov", "html": "cards/o1-title.html", "start": 0, "end": 3 },
        {
          "id": "o2",
          "src": "media/o2-lower-third.mov",
          "html": "cards/o2-lower-third.html",
          "start": 4.5,
          "end": 9,
          "x": 0.05,
          "y": 0.78,
          "width": 0.4
        }
      ]
    }
  ]
}
```

- Times are seconds. `src` is relative to the working directory; media and clip audio live in
  `media/` so the project folder is self-contained and the pool shows every clip. A file clip shows
  its `src` from `offset` (0 when absent) for `end - start` seconds at its `start`; the video track
  may hold several clips (a cut-up recording or different files) and the export plays them in
  `start` order.
- `resolution` (`"WxH"`, default `1920x1080`) is the export frame the user picks on the timeline:
  the recording is scaled to cover it, so a clip of a different shape is cropped rather than padded,
  and the preview shows what falls outside the frame blurred. Keep it as is. `fps` (default `30`) is
  the rate the export renders at; keep it as is too. Export writes `export/<output>`.
- A video clip's `scale` (a multiplier on the size that covers the frame) and `x`/`y` (the centre it
  is framed on, fractions of the frame) are how the user has framed the recording - they drag it in
  the preview and scroll to zoom, or press Fit and Fill. `keys` animates any of these across the clip
  with keyframes - `{"scale": [{"t": 0, "v": 1}, {"t": 2, "v": 1.4}], "x": [...], "y": [...]}`, each a
  channel of `{"t": <seconds from the clip's start>, "v": <value>}` sorted by `t`, held flat before
  the first key and after the last and linear between.
- A video clip's `speed` (default `1`, range `0.1`-`8`) retimes it and resizes it on the timeline the
  way every editor does - same source content, so `2` is fast-forward at half the length and `0.5` is
  slow motion at double. `speed_ease` is `"constant"` (default) or `"easeInOut"`, a self-contained
  bump that runs `1x` at the clip's edges and peaks at `speed` in the middle. Build a longer speed
  ramp by cutting the clip where the change should happen and setting each piece's `speed`. Leave     
  `scale`, `x`, `y`, `keys`, `speed`, `speed_ease` and `volume` exactly as they are unless the user    
  asks.                                                                                              
- An audio clip's `volume` (default `1`, range `0`-`2`) is that clip's own level on top of its      
  track's `gain`: the preview and the export apply both. It is the user's setting, so set it only   
  when they ask for a clip quieter or louder (music ducked under a voice, a quiet take lifted), and
  leave every other clip's as it is.
- `offset` (optional, seconds) is where a clip starts inside its `src` file; the desktop sets it when
  the user trims a clip's head on the timeline. Keep it as is, except when you cut or trim clips
  yourself (see Cuts and trims): then you set it.
- A spoken clip's `voice_sample` is the sample its wav was cloned from: set it to the sample you
  used every time you synthesize the clip, and leave other clips' as they are - the timeline colours
  each clip by it, so the user can see which voice every clip carries. The track's `voice_sample` is
  the user's pick for clips that have none of their own.
- A video clip with `avatar` is a talking clip: that avatar lip-synced to the spoken clip over the
  same range, with `id` `<spoken id>-avatar`, the spoken clip's `start` and `end`, and `src` the
  rendered `media/<stem>-<spoken id>-avatar.mp4`. Its `voice_sample` is the sample of the audio it was
  rendered from. The voice stays on the spoken clip - the export mixes that wav, not the mp4's own
  sound - so never set `source_audio: "keep"` on a timeline with avatar clips: the voice would play
  twice. See Avatars.
- `status: "draft"` means the clip needs (re)synthesis, or for an avatar clip a (re)render. Only
  touch draft clips; never regenerate a `done` clip the user did not ask about. Keep clip `id`s stable.
- A draft clip with non-empty `text` was written by the user: keep the text verbatim. Empty text
  means "suggest something for this range".
- Tracks are `video`, `audio`, `overlay` or `captions` (the older `voice` kind still loads as
  `audio`). On an audio track,
  a clip with `text` is spoken: you synthesize it. A clip with only `src` (music, SFX, a file the
  user dropped on the lane) is a plain file: never touch, move or regenerate it. The desktop's     
  export mixes every audio clip with its track `gain` and the clip's own `volume`, if it has one. Put spoken clips on the audio track that 
  already holds speech, or add one with `id: "voice"`; never invent plain-file clips.
- A captions track is on-screen text drawn onto the export: `kind: "captions"`, a `style` preset
  and an optional `position` (or `x`/`y`, the centre of the block as fractions of the frame, set by
  dragging it on the preview), with clips of `id`, `start`, `end`, `text` and optional `words`. One
  captions track per timeline; caption clips never carry `src` or `status`. See Captions.
- `source_audio` says what to do with the recording's own audio track: `transcribe` (reuse the
  user's own speech as the script and as the voice sample, then replace it), `mute` (drop
  it), or `keep` (mix it under the voice). Missing means: `transcribe` when the recording has
  speech, else `mute`; write the choice back so the desktop shows it.

## Steps

1. **Probe.** `ffmpeg -hide_banner -i <video> 2>&1 | grep -E 'Duration|Stream'`. The
   `Duration: HH:MM:SS.ms` line gives `duration` in seconds; a `Stream ... Audio:` line means the
   recording has sound.
2. **Keyframes.** Prefer scene changes; fall back to fixed sampling on static screens:

   ```sh
   mkdir -p tmp/frames
   ffmpeg -hide_banner -i <video> -vf "select='gt(scene,0.3)',showinfo,scale=640:-1" -fps_mode vfr tmp/frames/%03d.jpg 2> tmp/frames/showinfo.log
   grep -o 'pts_time:[0-9.]*' tmp/frames/showinfo.log
   ```

   The n-th `pts_time` is the timestamp of `tmp/frames/<n>.jpg`. Cap at about 40 frames: raise the
   threshold (0.4, 0.5) if there are more, or if there are fewer than 4 use
   `-vf "fps=1/5,scale=640:-1"` (one frame every 5 s) instead.

3. **Describe.** Call `ImageDecode` on every frame with the prompt
   "One sentence: what is the user doing on screen right now?" If you can see images, the frame
   itself comes back attached: describe it yourself in one sentence. Otherwise use the text
   description the tool returns. Keep the answers with their timestamps. Never open frames in a
   browser or guess their content.
   With `source_audio: transcribe`, also run the Source audio steps below; the transcript is the
   primary script and the frame descriptions only fill gaps.
4. **Plan.** Group consecutive frames that describe the same activity into segments. Each segment
   becomes a voice clip: `start` = first frame time, `end` = next segment's start (last one ends
   at `duration`). Write short, spoken-style text sized to the slot: about 2.5 words per second, so a
   6 s slot gets at most 15 words. Merge any existing draft clips from the user by their time range.
   Write `<stem>.timeline.json` with all voice clips `status: "draft"`.
5. **Synthesize.** Settle the voice first (see Picking the voice), then for every draft clip:
   `TextToSpeech { text, voice_sample: "<sample name>", output_path: "<stem>-<id>.wav" }`, where
   `<sample name>` is the library sample's bare name, or `voice.wav` for the recording.
   The tool reports the wav path (under `~/.infer/tts/`) and its duration. If the duration exceeds
   `end - start`, shorten the text and synthesize once more. Copy the wav into the project:
   `mkdir -p media && cp "<reported path>" "media/<stem>-<id>.wav"`, set `src` to
   `media/<stem>-<id>.wav`, `voice_sample` to the sample you cloned and `status: "done"`. Write the JSON after each clip so the desktop can
   show progress.
6. **Stop here.** Do not mux, render or export anything, and do not run ffmpeg on the output:
   the user reviews the clips on the timeline and presses Export, which renders the video
   deterministically from the JSON. Tell the user how many clips you placed, that every clip can be
   edited on the timeline, and that "redo the draft clips" regenerates only the edited ones.

## Source audio (re-voicing a spoken recording)

When `source_audio` is `transcribe`, or it is unset and the probe showed an `Audio:` stream:

1. Extract: `ffmpeg -y -i <video> -vn -ac 1 -ar 16000 tmp/audio.wav`.
2. Transcribe with timestamps: `~/.infer/bin/tools/whisper-cli -m ~/.infer/models/whisper/ggml-tiny.bin -f tmp/audio.wav -oj -of tmp/transcript`
   writes `tmp/transcript.json` with `transcription[].timestamps` / `offsets` (ms) and `text`. Use the
   largest ggml model present. If the transcript is empty or only noise, fall back to `mute` and
   say so; do not try to boost, filter or split the audio and transcribe again.
3. Segments: merge transcript lines into clips of one thought each (roughly 4-12 s), `start`/`end`
   from the offsets. Rewrite every clip's text into clean, simple spoken text: drop filler words, false
   starts and repetitions, fix grammar, keep the meaning, the order and the timing budget
   (2.5 words per second). Do not add facts the user did not say.
4. Voice sample: when Picking the voice landed on the recording, cut the cleanest 15-25 s stretch
   of continuous speech: `ffmpeg -y -i tmp/audio.wav -ss <start> -t <len> voice.wav`.
5. Continue with Synthesize, then stop; the export replaces the original track.

## Avatars (talking clips)

When the user asks for a talking avatar, a presenter or a talking head, make the voice as usual and
place a lip-synced clip of their avatar over it.

1. **Pick the avatar.** A clip's own `avatar` is the user's pick: use it and do not ask. Otherwise
   `ls ~/.infer/avatars/` and, when there are several, ask once with `AskUserQuestion` (one option per
   avatar, asked together with the voice question). One avatar: use it. None: stop and say so,
   avatars are created in Settings > Avatars.
2. **Script and voice.** Plan and synthesize the spoken clips as in steps 4 and 5 of Steps (and
   Picking the voice): the script and the voice live on the audio track as for any voiceover.
3. **Place.** For every spoken clip that should talk, add a video-track clip
   `{ "id": "<id>-avatar", "start", "end", "avatar": "<name>", "status": "draft" }` with the spoken
   clip's `start` and `end`. Clips on the video track must not overlap: without a recording the avatar
   clips fill the track; with one, put them where the user asked (an intro or outro in a gap) and cut
   the recording as in Cuts and trims only when they asked for the avatar over it.
4. **Render.** For every `draft` avatar clip:
   `TextToVideo { avatar: "<name>", audio: "<stem>-<id>.wav", output_path: "<stem>-<id>-avatar.mp4" }`,
   where `<stem>-<id>.wav` is the spoken clip's wav. `audio` takes a bare name, which resolves in the
   `TextToSpeech` output directory right after synthesis; for a wav that is only in `media/`, run
   `cp media/<stem>-<id>.wav .` first and `rm` that copy afterwards. Copy the reported mp4 into
   `media/<stem>-<id>-avatar.mp4`, set `src` to it, `avatar` and `voice_sample` to what you used and
   `status: "done"`, and write the JSON after each clip. A failed render comes back as a one-line
   error: stop and tell the user, never retry with another avatar or model.
5. **Keep them in step.** Re-synthesizing a spoken clip makes the avatar clip over the same range
   stale: set it to `draft` and render it again. Only `draft` avatar clips are rendered.
6. **Stop** as in step 6 of Steps.

## Captions

Add a captions track when the user asks for captions or subtitles, or asks for a clip "to post"
(short-form video is watched muted). Captions are independent of the voice: the export draws them
onto the frame and also writes a sidecar `.srt`.

1. **Text.** If the timeline already has spoken clips, their `text` is the script: reuse it, no
   transcription. Otherwise, with `source_audio: transcribe`, use `tmp/transcript.json` from the Source
   audio step. With neither, there is nothing to caption: say so instead of inventing lines.
2. **Chunk.** One caption per breath: about 5 s, at most ~12 words and two lines. Break at sentence
   ends and pauses, never mid-word and never mid-number. `start`/`end` come from the spoken clip or
   the transcript offsets; a long spoken clip splits into several captions inside its own range.
   Drop filler and trailing punctuation clutter, keep the words the user says.
3. **Style.** `classic` (a broadcast subtitle: white on a translucent box), `bold` (the short-form
   punch line: huge uppercase yellow with a thick outline), `highlight` (each word turns green as it
   is spoken and stays) or `karaoke` (words are dim until spoken, then white). Use `classic` unless
   the user asks for a word-by-word look or a short-form clip. `position` is `bottom` (default),
   `center` or `top`; leave `x`/`y` alone, the user sets them by dragging the captions on the
   preview and they win over `position`.
4. **Words.** `highlight` and `karaoke` need per-word timing; the other two ignore it. Only for
   those two, run a second pass for word-level splits over the wav the text came from - `tmp/audio.wav`
   for a transcript, `media/<stem>-<id>.wav` for a synthesized clip:
   `~/.infer/bin/tools/whisper-cli -m <model> -f <wav> -ml 1 -oj -of tmp/words-<id>`. Write `words` as
   absolute seconds on the timeline (`{ "text", "start", "end" }`, offset by the clip's `start` when
   the wav is a single clip), inside each caption's own range. Without `words` both presets fall
   back to the whole line, so skipping the pass is safe but loses the effect.
5. **Stop.** Write the JSON and stop, as in step 6 of Steps. The user reviews the captions on the
   timeline and presses Export.

Editing a captions track: read the file first, keep clip `id`s stable and change only what the user
asked about. A misheard word is one clip's `text`, not a reason to rebuild the track. The user also
adds caption clips on the timeline: a caption with empty `text` is a range they want filled, one
with text is theirs and stays verbatim.

## Redo drafts

When asked to redo or regenerate: ask which voice to use (see Picking the voice), read the JSON,
run step 5 for `draft` clips only, render `draft` avatar clips (including the ones step 5 made
stale) as in Avatars, then stop as in step 6. Never touch `done` clips the user did not edit. When asked for one clip by id, the desktop
has already marked that clip `draft` with the user's edited text: synthesize exactly that clip with
that text, keep its `id`, `start` and `end`, and leave everything else alone.

## Cuts and trims

When the user asks to cut out a section ("cut the part from 10 s to 20 s") or trim a clip's head or
  tail ("trim the first 5 seconds"), edit the JSON: no ffmpeg on the sources, nothing is
  re-encoded. Edits are non-destructive and never ripple: a clip keeps `start` (timeline position),
  `end` (timeline out-point) and `offset` (source in-point); every other clip and track keeps its
  times, a gap stays, and the export freeze-frames video gaps (audio and overlays at those times
  still play).

- Trim the head of a file clip: raise `start` and `offset` by the same amount, so the content stays
  where it is on the timeline. Trim the tail: lower `end`, never past the source's duration (probe
  it with `ffmpeg -hide_banner -i <src> 2>&1 | grep Duration`).
- Split a clip at time `t` (`start < t < end`): the first piece keeps the `id` and gets
  `end: t`; the second gets a new unique `id`, `start: t`, the old `end`, and `offset` raised by
  `t - start`, so its source continues seamlessly at the cut.
- Remove a section `[a, b]` from a track: for every clip overlapping it, keep the head part (its
  `end` becomes `a`) and split the part after `b` off as above (its `start` becomes `b`, `offset`
  advanced by `b - a`); delete clips the section covers completely. Never move other clips to
  close the gap.
- Mark spoken clips you split, shortened or created `status: "draft"` (keep the `text` on the piece
  that still says it, empty on the rest) and synthesize the drafts as in step 5, then stop as in
  step 6.

## Overlay cards

A card is an HTML composition rendered by HyperFrames to a `.mov` with alpha and placed as a clip on
an `overlay` track; the desktop shows it over the video in the preview and composites it into the
export. `/hyperframes` (and its `motion-graphics` workflow) own how a composition is written; this
section owns where it goes.

Never install or set anything up: no `npm install`, `brew`, `hyperframes init` or
`hyperframes browser ensure`. Check, and stop at the first missing piece, naming it: the `hyperframes`
skill is installed at `~/.infer/skills/hyperframes/SKILL.md` (`infer skills install hyperframes motion-graphics`
or Settings > Skills); `node --version` is v22 or newer; `ffmpeg -hide_banner -filters | grep -c ' overlay '`
prints `1`. If a render fails because HyperFrames or its browser is missing, tell the user to run
`npx hyperframes browser ensure` once in a terminal; do not run it for them.

- Overlay clips: `src` is the rendered card under `media/`, `html` the composition under `cards/` it
  came from (keep both so a card can be re-rendered instead of rewritten), `start`/`end` on the
  video, and optional `x`, `y`, `width`, `height` as fractions (0-1) of the export frame, top-left
  origin; `width` alone keeps the aspect ratio, none means full frame width. Never put anything else
  on an overlay track. Use one track `id: "cards"` unless cards overlap in time; clips on one track
  must not overlap; keep ids stable and read the file back before changing it, the user moves and
  trims cards on the timeline.
- Frame: the export is `resolution` (`1920x1080` by default, `1080x1920` or `1350x1350`; the user
  picks it, never change it). A full-frame card is a composition of exactly that size; a smaller
  card keeps the same pixel density (a 1920-wide frame with `width: 0.4` is a 768 px wide card).
  The composition's duration is `end - start`.
- Write `cards/<id>-<kind>.html` (`mkdir -p cards media`). Everything visible sits inside one root
  `<div id="stage" data-composition-id="<id>-<kind>" data-start="0" data-duration="<s>" data-fps="30"
  data-width="<W>" data-height="<H>" style="position:relative;width:<W>px;height:<H>px">` in
  `<body>`; attributes on `<body>` are ignored and the render fails with "Composition duration is 0".
  Cards that animate with CSS only also carry `data-no-timeline` on that root so the renderer does
  not wait for a GSAP timeline. No files outside the working directory, no network fonts.
- Render: `npx --yes hyperframes render -c cards/<id>-<kind>.html --format mov -o media/<id>-<kind>.mov --quiet`.
  Always `mov` (ProRes 4444): the desktop's preview drops the alpha of a WebM, and mp4 has none.
  Check the duration and size with
  `ffmpeg -hide_banner -i media/<id>-<kind>.mov 2>&1 | grep -E 'Duration|Stream'`; render again
  after fixing the HTML if it is off. Keep cards under 10 s, ProRes is large.
- Place the clip on the overlay track (create the track if missing), write the timeline, and stop as
  in step 6: no ffmpeg on the video, no export. To change a card, edit the file named by its `html`,
  render to the same `src`, leave `start`/`end` alone unless asked; placement changes need no render.

## Notes

- Never use `open` or play audio yourself; the desktop renders media inline.
- Remove `tmp/frames/` with `rm -r tmp/frames` at the end unless the user wants the stills; leave the
  other scratch files in place.
- Voice quality depends on the sample: one speaker, no background noise, no music.
