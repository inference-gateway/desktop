import { describe, expect, test } from "bun:test";
import { addMarker, clipLayout, draftCount, parseTimeline, removeClip, setClipText, videoSource } from "./timeline";

const SAMPLE = JSON.stringify({
  version: 1,
  duration: 20,
  output: "demo.narrated.mp4",
  tracks: [
    { id: "video", kind: "video", clips: [{ id: "v1", src: "demo.mov", start: 0, end: 20 }] },
    {
      id: "voice",
      kind: "narration",
      clips: [
        { id: "s2", start: "10", end: 20, text: "second", status: "done" },
        { id: "s1", start: 0, end: 10, text: "first", status: "done" },
      ],
    },
  ],
});

describe("parseTimeline", () => {
  test("coerces numbers, sorts clips and keeps the video source", () => {
    const t = parseTimeline(SAMPLE);
    expect(t.duration).toBe(20);
    expect(videoSource(t)).toBe("demo.mov");
    expect(t.tracks[1].clips.map((c) => c.id)).toEqual(["s1", "s2"]);
    expect(t.tracks[1].clips[1].start).toBe(10);
  });

  test("rejects files without tracks and derives duration from clips", () => {
    expect(() => parseTimeline("{}")).toThrow();
    expect(parseTimeline('{"tracks":[{"kind":"video","clips":[{"start":0,"end":7}]}]}').duration).toBe(7);
  });
});

describe("edits", () => {
  test("setClipText marks the clip draft", () => {
    const t = setClipText(parseTimeline(SAMPLE), "voice", "s1", "changed");
    expect(t.tracks[1].clips[0]).toMatchObject({ text: "changed", status: "draft" });
    expect(t.tracks[1].clips[1].status).toBe("done");
    expect(draftCount(t)).toBe(1);
  });

  test("addMarker inserts a draft clip capped at the next clip", () => {
    const t = addMarker(parseTimeline(SAMPLE), 8);
    const clips = t.tracks[1].clips;
    expect(clips.map((c) => c.id)).toEqual(["s1", "m3", "s2"]);
    expect(clips[1]).toMatchObject({ start: 8, end: 10, status: "draft" });
  });

  test("addMarker creates the narration track when missing", () => {
    const t = addMarker(parseTimeline('{"duration":30,"tracks":[]}'), 3, "hello");
    expect(t.tracks[0]).toMatchObject({ kind: "narration", clips: [{ start: 3, end: 8, text: "hello" }] });
  });

  test("removeClip drops the clip", () => {
    expect(removeClip(parseTimeline(SAMPLE), "voice", "s1").tracks[1].clips.map((c) => c.id)).toEqual(["s2"]);
  });
});

test("clipLayout maps seconds to percentages", () => {
  expect(clipLayout({ id: "x", start: 5, end: 10 }, 20)).toEqual({ left: "25%", width: "25%" });
  expect(clipLayout({ id: "x", start: 0, end: 1 }, 0)).toEqual({ left: "0%", width: "0%" });
});
