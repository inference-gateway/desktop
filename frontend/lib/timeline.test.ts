import { describe, expect, test } from "bun:test";
import {
  addMarker,
  addTrack,
  clipLayout,
  emptyTimeline,
  draftCount,
  parseTimeline,
  removeClip,
  setClipText,
  videoSource,
} from "./timeline";

const SAMPLE = JSON.stringify({
  version: 1,
  duration: 20,
  output: "demo.with-voice.mp4",
  tracks: [
    { id: "video", kind: "video", clips: [{ id: "v1", src: "demo.mov", start: 0, end: 20 }] },
    {
      id: "voice",
      kind: "voice",
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
    expect(t.source_audio).toBeUndefined();
    expect(parseTimeline('{"source_audio":"keep","tracks":[]}').source_audio).toBe("keep");
    expect(parseTimeline('{"source_audio":"bogus","tracks":[]}').source_audio).toBeUndefined();
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

  test("addMarker creates the voice track when missing", () => {
    const t = addMarker(parseTimeline('{"duration":30,"tracks":[]}'), 3, "hello");
    expect(t.tracks[0]).toMatchObject({ kind: "voice", clips: [{ start: 3, end: 8, text: "hello" }] });
  });

  test("removeClip drops the clip", () => {
    expect(removeClip(parseTimeline(SAMPLE), "voice", "s1").tracks[1].clips.map((c) => c.id)).toEqual(["s2"]);
  });
});

test("clipLayout maps seconds to percentages", () => {
  expect(clipLayout({ id: "x", start: 5, end: 10 }, 20)).toEqual({ left: "25%", width: "25%" });
  expect(clipLayout({ id: "x", start: 0, end: 1 }, 0)).toEqual({ left: "0%", width: "0%" });
});

test("addTrack layers lanes with unique ids", () => {
  const t = addTrack(addTrack(emptyTimeline(), "audio"), "video");
  expect(t.tracks.map((tr) => tr.id)).toEqual(["video", "audio", "audio2", "video2"]);
});

test("addClip places at the drop time or after the last clip when taken", () => {
  const a = addClip(emptyTimeline(), "video", "intro.mp4", 6, 0);
  expect(a.tracks[0].clips[0]).toMatchObject({ id: "v1", start: 0, end: 6, src: "intro.mp4" });
  expect(a.duration).toBe(6);
  const b = addClip(a, "video", "demo.mov", 4, 2);
  expect(b.tracks[0].clips.map((c) => [c.id, c.start, c.end])).toEqual([
    ["v1", 0, 6],
    ["v2", 6, 10],
  ]);
  expect(addClip(a, "nope", "x.mp4", 1, 0)).toBe(a);
});
