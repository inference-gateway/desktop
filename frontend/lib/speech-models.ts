export const LOCAL_SPEECH_MODEL = "local/qwen3-tts";

export type SpeechModel = { id: string; label: string };

// shortcut: a fixed list, because the gateway's model listing cannot tell a speech model from a
// music one. Read it from the gateway once models carry that.
export const SPEECH_MODELS: SpeechModel[] = [
  { id: LOCAL_SPEECH_MODEL, label: "Qwen3-TTS - local, free, runs on this computer" },
  { id: "elevenlabs/eleven_v4_turbo", label: "ElevenLabs v4 Turbo - hosted, fastest, recommended" },
  { id: "elevenlabs/eleven_v4", label: "ElevenLabs v4 - hosted, highest quality" },
  { id: "elevenlabs/eleven_v3", label: "ElevenLabs v3 - hosted" },
];

/** The model speech is synthesized with: an unset one is the CLI's local default. */
export function effectiveSpeechModel(configured: string): string {
  return configured.trim() || LOCAL_SPEECH_MODEL;
}

export function isLocalSpeechModel(configured: string): boolean {
  return effectiveSpeechModel(configured).startsWith("local/");
}

/** The picker's options, keeping a model set by hand in config.yaml selectable. */
export function speechModelOptions(configured: string): SpeechModel[] {
  const current = effectiveSpeechModel(configured);
  return SPEECH_MODELS.some((m) => m.id === current)
    ? SPEECH_MODELS
    : [...SPEECH_MODELS, { id: current, label: current }];
}

export function speechModelLabel(configured: string): string {
  const current = effectiveSpeechModel(configured);
  return SPEECH_MODELS.find((m) => m.id === current)?.label ?? current;
}
