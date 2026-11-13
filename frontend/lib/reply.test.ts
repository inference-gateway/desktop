import { expect, test } from "bun:test";
import { replyMarker, splitReply, withReply } from "./reply";

test("a message reply quotes a flattened excerpt", () => {
  expect(replyMarker("message", "Rows 1-2\n\nare in.  ")).toBe("[Replying to message: Rows 1-2 are in.]");
});

test("a long message is cut at the excerpt limit", () => {
  const marker = replyMarker("message", "x".repeat(400));
  expect(marker).toBe(`[Replying to message: ${"x".repeat(280)}…]`);
});

test("an audio reply names the file", () => {
  expect(replyMarker("audio", "/home/infer/.infer/tmp/media/tts/line-02.wav")).toBe(
    "[Replying to audio: /home/infer/.infer/tmp/media/tts/line-02.wav]",
  );
});

test("the marker goes above the prompt and stands alone without one", () => {
  expect(withReply("[Replying to audio: a.wav]", "make it slower")).toBe(
    "[Replying to audio: a.wav]\n\nmake it slower",
  );
  expect(withReply("[Replying to audio: a.wav]", "")).toBe("[Replying to audio: a.wav]");
  expect(withReply(null, "hi")).toBe("hi");
});

test("a sent prompt splits into the quoted reply and the user's text", () => {
  expect(splitReply("[Replying to audio: /tmp/a.wav]\n\nslower")).toEqual({ quote: "/tmp/a.wav", rest: "slower" });
  expect(splitReply("[Replying to message: Done.]")).toEqual({ quote: "Done.", rest: "" });
  expect(splitReply("plain prompt")).toEqual({ quote: null, rest: "plain prompt" });
});
