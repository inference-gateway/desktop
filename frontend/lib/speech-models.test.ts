import { expect, test } from "bun:test";
import {
  LOCAL_SPEECH_MODEL,
  SPEECH_MODELS,
  effectiveSpeechModel,
  isLocalSpeechModel,
  speechModelLabel,
  speechModelOptions,
  speechRestartsGateway,
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

test("the gateway restarts when speech turns on or off, or its download setting changes while on", () => {
  const off = { text_to_speech_enabled: false, text_to_speech_auto_download: true };
  const on = { text_to_speech_enabled: true, text_to_speech_auto_download: true };
  expect(speechRestartsGateway(off, on)).toBe(true);
  expect(speechRestartsGateway(on, off)).toBe(true);
  expect(speechRestartsGateway(on, on)).toBe(false);
  expect(speechRestartsGateway(on, { ...on, text_to_speech_auto_download: false })).toBe(true);
  expect(speechRestartsGateway(off, { ...off, text_to_speech_auto_download: false })).toBe(false);
});
