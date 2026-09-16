import { describe, expect, test } from "bun:test";
import {
  addClip,
  moveClip,
  rulerStep,
  snapPoints,
  snapTime,
  trimClip,
  spokenCount,
  spokenTrack,
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
      kind: "audio",
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

  test("addMarker creates an audio track when missing", () => {
    const t = addMarker(parseTimeline('{"duration":30,"tracks":[]}'), 3, "hello");
    expect(t.tracks[0]).toMatchObject({ kind: "audio", clips: [{ start: 3, end: 8, text: "hello" }] });
  });

  test("legacy voice tracks load as audio and spoken clips are the ones with text", () => {
    const t = parseTimeline(
      '{"duration":10,"tracks":[{"id":"v","kind":"voice","clips":[{"id":"a","start":0,"end":2,"text":"hi"},{"id":"b","start":2,"end":4,"src":"m.mp3"}]}]}',
    );
    expect(t.tracks[0].kind).toBe("audio");
    expect(spokenCount(t)).toBe(1);
    expect(spokenTrack(t)?.id).toBe("v");
  });

  test("removeClip drops the clip", () => {
    expect(removeClip(parseTimeline(SAMPLE), "voice", "s1").tracks[1].clips.map((c) => c.id)).toEqual(["s2"]);
  });
});

test("clipLayout maps seconds to pixels at the zoom", () => {
  expect(clipLayout({ id: "c", start: 2, end: 5 }, 10)).toEqual({ left: 20, width: 30 });
  expect(clipLayout({ id: "c", start: 2, end: 2.01 }, 10).width).toBe(2);
  expect(rulerStep(10)).toBe(10);
  expect(rulerStep(100)).toBe(1);
  expect(rulerStep(0.01)).toBe(600);
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

test("moveClip slides between neighbours and grows the timeline", () => {
  const t = addClip(
    addClip(addClip(emptyTimeline(), "audio", "a.mp3", 4, 0), "audio", "b.mp3", 2, 6),
    "audio",
    "c.mp3",
    3,
    10,
  );
  const starts = (x: ReturnType<typeof moveClip>) => x.tracks[1].clips.map((c) => [c.id, c.start, c.end]);
  expect(starts(moveClip(t, "audio", "a2", 7))).toEqual([
    ["a1", 0, 4],
    ["a2", 7, 9],
    ["a3", 10, 13],
  ]);
  expect(starts(moveClip(t, "audio", "a2", 1))[1]).toEqual(["a2", 4, 6]);
  expect(starts(moveClip(t, "audio", "a2", 30))[1]).toEqual(["a2", 8, 10]);
  const past = moveClip(t, "audio", "a3", 30);
  expect(starts(past)[2]).toEqual(["a3", 30, 33]);
  expect(past.duration).toBe(33);
  expect(moveClip(t, "audio", "a1", -5).tracks[1].clips[0].start).toBe(0);
  expect(moveClip(t, "audio", "zzz", 1)).toBe(t);
});

test("trimClip moves the head with its offset, stops at neighbours and the source end", () => {
  const t = addClip(addClip(emptyTimeline(), "audio", "a.mp3", 4, 0), "audio", "b.mp3", 6, 6);
  const head = trimClip(t, "audio", "a2", "start", 8);
  expect(head.tracks[1].clips[1]).toMatchObject({ start: 8, end: 12, offset: 2 });
  const back = trimClip(head, "audio", "a2", "start", 0);
  expect(back.tracks[1].clips[1]).toMatchObject({ start: 6, end: 12 });
  expect(back.tracks[1].clips[1].offset).toBeUndefined();
  expect(trimClip(t, "audio", "a1", "end", 30).tracks[1].clips[0].end).toBe(6);
  expect(trimClip(t, "audio", "a1", "end", 30, 5).tracks[1].clips[0].end).toBe(5);
  expect(trimClip(t, "audio", "a2", "end", 0).tracks[1].clips[1].end).toBe(6.25);
  const longer = trimClip(t, "audio", "a2", "end", 20);
  expect(longer.duration).toBe(20);
});

test("snapTime picks the nearest point within tolerance", () => {
  const t = addClip(addClip(emptyTimeline(), "audio", "a.mp3", 4, 0), "audio", "b.mp3", 6, 6);
  expect(snapPoints(t, "a2", 9).sort((a, b) => a - b)).toEqual([0, 0, 4, 9]);
  expect(snapTime(3.9, [0, 4, 9], 0.2)).toBe(4);
  expect(snapTime(3.5, [0, 4, 9], 0.2)).toBe(3.5);
  expect(snapTime(4.1, [4, 4.15], 0.2)).toBe(4.15);
});
