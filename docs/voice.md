# Voice samples

Settings > **Voice samples** keeps reference WAV recordings of you that the agent clones when it speaks - the voice of every spoken clip in Content projects, avatar tests and any "read this in my voice" ask in the chat. The library lives flat in `~/.infer/voice/samples`, and the agent uses one by its full file name, e.g. "read this in the voice of my-voice.wav".

## Building the library

- **Record** - type a sample name (default `my-voice`) and press the microphone button. It records the raw microphone (the browser's own processing smears a voice), shows the level while you speak, and saves the take as WAV after trimming silence. A recording longer than 30 seconds stops itself.
- **Import** - **Add sample** picks a WAV anywhere and copies it into the library under its file name. Re-adding the same name overwrites the old take; the trash icon deletes one.
- **Quality** - one speaker, a quiet room, a consistent distance from the mic, and nothing but the sample. Under 10 seconds is too short, and the card warns about takes that are too quiet or clipping. Aim for 10-13 seconds per sample, and four different readings clone best: the tab suggests one per style - Conversational, Enthusiastic, Thoughtful and curious, and Confident and clear - click a style to use it as the sample name. The counter shows `N of 4 recommended samples`.

## The speech model

The tab's speech settings are the gateway's `text_to_speech` settings, saved the moment you change them; turning speech on or off restarts the gateway.

- **Enable Text to Speech** gives the agent the CLI's `TextToSpeech` tool.
- **Speech model** (`text_to_speech.model`) picks who speaks: **Qwen3-TTS - local, free, runs on this computer** (the default, and the only one that stays local) or an ElevenLabs one - **v4 Turbo** (fastest), **v4** (highest quality) or **v3**. The local model clones a voice sample when the agent names one. An ElevenLabs model needs its key under Settings > API Keys and clones a named sample into your ElevenLabs account on first use, where it stays until you delete it; a restricted key needs the Voices permission, read and write, as well as Text to Speech. With no sample named, it speaks in the tab's **Voice** id.
- **Download the local model when it is missing** - the local model is about 1.4 GB and the gateway fetches it on first use when this is on; leave it off to download it yourself first.
- **Save speech to** - where WAVs land, `~/.infer/tmp/media/tts` by default. The chat only plays speech saved in the default folder, so leave it empty unless you have a reason.
- **Ask before the agent speaks** - approval for every synthesis, on top of the normal tool approval.

## Test voice

**Test** on a sample reads the **Test text** (editable, with a default already filled in) with the chosen model, exactly the call an agent makes, and plays "How an agent sounds in this voice" under the sample. One test file per sample, `~/.infer/tmp/media/tts/voice-test-<name>.wav`, replaced on every test. A test needs Text to speech on and a model that clones - the tab says when the speech model does not clone voice samples, and notes when the first test of a sample stores it as a voice in your ElevenLabs account.

Samples show up on every audio track's **Voice** dropdown on the timeline, and Avatars' **Test** picks a sample's voice the same way - see [Avatars](avatars.md). [examples/voiceover](../examples/voiceover) uses one from a screen recording to an exported MP4.
