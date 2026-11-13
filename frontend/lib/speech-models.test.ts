import { expect, test } from "bun:test";
import {
  LOCAL_SPEECH_MODEL,
  SPEECH_MODELS,
  effectiveSpeechModel,
  isLocalSpeechModel,
  speechModelLabel,
  speechModelOptions,
} from "./speech-models";

test("an unset speech model is the local default", () => {
  expect(effectiveSpeechModel("")).toBe(LOCAL_SPEECH_MODEL);
  expect(isLocalSpeechModel("")).toBe(true);
  expect(isLocalSpeechModel("elevenlabs/eleven_v4")).toBe(false);
});

test("a model set by hand stays selectable and labelled by its id", () => {
  expect(speechModelOptions("elevenlabs/eleven_v4")).toBe(SPEECH_MODELS);
  expect(speechModelOptions("openai/gpt-4o-mini-tts").at(-1)).toEqual({
    id: "openai/gpt-4o-mini-tts",
    label: "openai/gpt-4o-mini-tts",
  });
  expect(speechModelLabel("openai/gpt-4o-mini-tts")).toBe("openai/gpt-4o-mini-tts");
});
