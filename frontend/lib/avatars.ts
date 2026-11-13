import type { DesktopConfig, VoiceSample } from "@/lib/tauri";

// About five seconds of speech: a test renders a clip as long as its line, and
// the video provider bills by that length.
export const AVATAR_TEST_LINE = "Hi, I am your presenter. This is how I look and sound on screen.";

type TalkingSetup = Pick<DesktopConfig, "text_to_speech_enabled" | "text_to_video_enabled">;

// Why an avatar cannot be tested yet, or "" when it can: a talking clip needs
// both the voice and the video tool.
export function avatarTestBlocker(setup: TalkingSetup | null): string {
  if (!setup) return "Loading settings...";
  if (!setup.text_to_speech_enabled)
    return "Text to speech is off. Turn it on in Settings > Voice samples to test an avatar.";
  if (!setup.text_to_video_enabled) return "Text to video is off. Turn it on in Settings > General to test an avatar.";
  return "";
}

export const SETTINGS_VOICE = "";

// The voice a test starts on: the one the speech settings name, else the first
// voice sample. A cloning model without a voice of its own cannot speak, so
// the settings voice is no default when it is empty and a sample exists.
export function defaultTestVoice(settingsVoice: string, samples: VoiceSample[]): string {
  if (settingsVoice.trim() !== "" || samples.length === 0) return SETTINGS_VOICE;
  return samples[0].name;
}

export type LipSyncModel = { id: string; label: string };

const DEFAULT_LIP_SYNC_MODEL = "elevenlabs/creatify-aurora";

// shortcut: a fixed list, because the gateway's model listing does not say which video models
// lip-sync. Read it from the gateway once models carry that.
export const LIP_SYNC_MODELS: LipSyncModel[] = [
  { id: DEFAULT_LIP_SYNC_MODEL, label: "ElevenLabs - Creatify Aurora, needs a Pro plan" },
  { id: "fal/fal-ai/creatify/aurora", label: "fal - Creatify Aurora, billed per second of video" },
];

/** The model an avatar is lip-synced with: an unset one is the CLI's default. */
export function effectiveLipSyncModel(configured: string): string {
  return configured.trim() || DEFAULT_LIP_SYNC_MODEL;
}

/** The picker's options, keeping a model set by hand in config.yaml selectable. */
export function lipSyncModelOptions(configured: string): LipSyncModel[] {
  const current = effectiveLipSyncModel(configured);
  return LIP_SYNC_MODELS.some((m) => m.id === current)
    ? LIP_SYNC_MODELS
    : [...LIP_SYNC_MODELS, { id: current, label: current }];
}
