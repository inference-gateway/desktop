import { expect, test } from "bun:test";
import { SETTINGS_VOICE, avatarTestBlocker, defaultTestVoice } from "./avatars";

test("an avatar test needs both the voice and the video tool", () => {
  expect(avatarTestBlocker(null)).toBe("Loading settings...");
  expect(avatarTestBlocker({ text_to_speech_enabled: false, text_to_video_enabled: true })).toContain(
    "Text to speech is off",
  );
  expect(avatarTestBlocker({ text_to_speech_enabled: true, text_to_video_enabled: false })).toContain(
    "Text to video is off",
  );
  expect(avatarTestBlocker({ text_to_speech_enabled: true, text_to_video_enabled: true })).toBe("");
});

test("a test starts on the settings voice, or on the first sample when the settings name none", () => {
  const samples = [
    { name: "confident.wav", path: "/s/confident.wav" },
    { name: "warm.wav", path: "/s/warm.wav" },
  ];
  expect(defaultTestVoice("rachel", samples)).toBe(SETTINGS_VOICE);
  expect(defaultTestVoice("", samples)).toBe("confident.wav");
  expect(defaultTestVoice("  ", [])).toBe(SETTINGS_VOICE);
});
