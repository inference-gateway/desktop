import { describe, expect, test } from "bun:test";
import {
  CAPTION_STYLES,
  addClip,
  moveClip,
  rulerStep,
  snapPoints,
  snapTime,
  splitClip,
  serializeTimeline,
  trimClip,
  spokenCount,
  spokenTrack,
  addEmptyClip,
  addTrack,
  captionStyle,
  captionTrack,
  clipSample,
  sampleColour,
  setClipSample,
  moveCaptions,
  clipLayout,
  emptyTimeline,
  draftCount,
  frameAspect,
  laneOrder,
  overlayCount,
  parseTimeline,
  removeClip,
  setClipText,
  speakClip,
  videoSource,
  MAX_SPEED,
  sampleKf,
  framingAt,
  speedAt,
  avgSpeed,
  sourceConsumed,
  sourceTimeAt,
  setKf,
  setSpeed,
  toggleKf,
  moveKf,
  clearKeys,
  removeKf,
  type Clip,
  type Timeline,
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
    {
      id: "cards",
      kind: "overlay",
      clips: [
        { id: "o1", src: "media/title.webm", html: "cards/title.html", start: 1, end: 4, x: 0.1, y: "0.8", width: 0.5 },
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
    expect(parseTimeline('{"resolution":"1080x1920","tracks":[]}').resolution).toBe("1080x1920");
    expect(parseTimeline('{"resolution":"tall","tracks":[]}').resolution).toBeUndefined();
    expect(frameAspect(parseTimeline('{"resolution":"1350x1350","tracks":[]}'))).toBe(1);
    expect(frameAspect(parseTimeline('{"tracks":[]}'))).toBeCloseTo(16 / 9);
  });

  test("keeps overlay clips with their placement", () => {
    const t = parseTimeline(SAMPLE);
    expect(t.tracks[2].kind).toBe("overlay");
    expect(t.tracks[2].clips[0]).toEqual({
      id: "o1",
      start: 1,
      end: 4,
      offset: undefined,
      src: "media/title.webm",
      text: undefined,
      status: undefined,
      x: 0.1,
      y: 0.8,
      width: 0.5,
      height: undefined,
      html: "cards/title.html",
    });
    expect(overlayCount(t)).toBe(1);
    expect(laneOrder(t.tracks).map((tr) => tr.id)).toEqual(["cards", "video", "voice"]);
    const moved = moveClip(t, "cards", "o1", 2.5).tracks[2].clips[0];
    expect(moved).toMatchObject({ start: 2.5, end: 5.5, x: 0.1 });
    expect(trimClip(t, "cards", "o1", "end", 6).tracks[2].clips[0].end).toBe(6);
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

  test("addEmptyClip drafts a spoken clip in the first gap after the playhead", () => {
    const gappy = parseTimeline(
      '{"duration":30,"tracks":[{"id":"voice","kind":"audio","clips":[{"id":"s1","start":0,"end":10}]}]}',
    );
    const t = addEmptyClip(gappy, "voice", 8);
    expect(t.tracks[0].clips.map((c) => [c.id, c.start, c.end, c.status])).toEqual([
      ["s1", 0, 10, undefined],
      ["s2", 10, 15, "draft"],
    ]);
  });

  test("addEmptyClip appends past a full lane and grows the timeline", () => {
    const t = addEmptyClip(parseTimeline(SAMPLE), "voice", 8);
    const clips = t.tracks[1].clips;
    expect(clips.map((c) => [c.id, c.start, c.end])).toEqual([
      ["s1", 0, 10],
      ["s2", 10, 20],
      ["s3", 20, 25],
    ]);
    expect(t.duration).toBe(25);
  });

  test("addEmptyClip ignores an unknown track", () => {
    const t = parseTimeline(SAMPLE);
    expect(addEmptyClip(t, "nope", 3)).toBe(t);
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

const CAPTIONS = JSON.stringify({
  version: 1,
  duration: 12,
  tracks: [
    { id: "video", kind: "video", clips: [{ id: "v1", src: "demo.mov", start: 0, end: 12 }] },
    {
      id: "subs",
      kind: "captions",
      style: "karaoke",
      position: "top",
      clips: [
        {
          id: "c1",
          start: 0,
          end: 3,
          text: "hello there world",
          words: [
            { text: "hello", start: 0, end: 1 },
            { text: "there", start: 1, end: 2 },
            { text: "world", start: 2, end: 2.8 },
          ],
        },
      ],
    },
  ],
});

test("captions track parses, round-trips and falls back to the default preset", () => {
  const t = parseTimeline(CAPTIONS);
  const tr = captionTrack(t)!;
  expect(tr.style).toBe("karaoke");
  expect(tr.position).toBe("top");
  expect(tr.clips[0].words).toEqual([
    { text: "hello", start: 0, end: 1 },
    { text: "there", start: 1, end: 2 },
    { text: "world", start: 2, end: 2.8 },
  ]);
  expect(captionStyle(tr.style)).toBe("karaoke");
  expect(captionStyle("nope")).toBe(CAPTION_STYLES[0].value);
  expect(captionStyle(undefined)).toBe(CAPTION_STYLES[0].value);
  expect(laneOrder(t.tracks).map((tr) => tr.id)).toEqual(["subs", "video"]);
  const round = parseTimeline(serializeTimeline(t));
  expect(round.tracks[1].clips[0]).toMatchObject({ id: "c1", start: 0, end: 3, words: tr.clips[0].words });
  const moved = moveClip(t, "subs", "c1", 1);
  expect(moved.tracks[1].clips[0]).toMatchObject({ start: 1, end: 4, words: tr.clips[0].words });
  expect(setClipText(t, "subs", "c1", "fixed").tracks[1].clips[0].status).toBeUndefined();
  expect(draftCount(t)).toBe(0);
  expect(spokenCount(t)).toBe(0);
});

test("trimClip does not pull a spoken clip's tail back to the length of its wav", () => {
  const t = parseTimeline(
    '{"duration":30,"tracks":[{"id":"voice","kind":"audio","clips":[{"id":"s1","start":2,"end":8,"text":"hello","src":"media/s1.wav"}]}]}',
  );
  const wav = 4.5;
  const out = trimClip(t, "voice", "s1", "end", 9, wav);
  expect(out.tracks[0].clips[0].end).toBe(9);

  const music = parseTimeline(
    '{"duration":30,"tracks":[{"id":"music","kind":"audio","clips":[{"id":"a1","start":2,"end":8,"src":"media/a.mp3"}]}]}',
  );
  expect(trimClip(music, "music", "a1", "end", 9, wav).tracks[0].clips[0].end).toBe(6.5);

  const head = trimClip(t, "voice", "s1", "start", 0.5, wav).tracks[0].clips[0];
  expect(head).toMatchObject({ start: 0.5, end: 8 });
  expect(head.offset).toBeUndefined();
  expect(trimClip(music, "music", "a1", "start", 0.5, wav).tracks[0].clips[0].start).toBe(2);
});

test("moveCaptions places the block by its centre and clears back to position", () => {
  const t = parseTimeline(CAPTIONS);
  const moved = moveCaptions(t, "subs", 0.25, 0.8);
  expect(captionTrack(moved)).toMatchObject({ x: 0.25, y: 0.8, position: "top" });
  expect(captionTrack(parseTimeline(serializeTimeline(moved)))).toMatchObject({ x: 0.25, y: 0.8 });
  expect(captionTrack(moveCaptions(moved, "subs", -0.5, 2.4))).toMatchObject({ x: 0, y: 1 });
  const cleared = moveCaptions(moved, "subs");
  expect(cleared.tracks.map((tr) => [tr.x, tr.y])).toEqual([
    [undefined, undefined],
    [undefined, undefined],
  ]);
});

test("addEmptyClip adds captions that never overlap and are not spoken", () => {
  const empty = addTrack(parseTimeline('{"duration":30,"tracks":[]}'), "captions");
  const t = addEmptyClip(empty, "captions", 3);
  expect(captionTrack(t)!.clips[0]).toEqual({ id: "c1", start: 3, end: 8, text: "" });
  expect(spokenCount(t)).toBe(0);
  expect(draftCount(t)).toBe(0);

  const two = addEmptyClip(t, "captions", 1);
  expect(captionTrack(two)!.clips.map((c) => [c.id, c.start, c.end])).toEqual([
    ["c2", 1, 3],
    ["c1", 3, 8],
  ]);

  const three = addEmptyClip(two, "captions", 4);
  expect(captionTrack(three)!.clips.map((c) => [c.id, c.start, c.end])).toEqual([
    ["c2", 1, 3],
    ["c1", 3, 8],
    ["c3", 8, 13],
  ]);
});

test("speakClip drafts a spoken clip from a caption, or updates the one at the same range", () => {
  const t = parseTimeline(CAPTIONS);
  const spoken = speakClip(t, "subs", "c1");
  const audio = spoken.tracks.find((tr) => tr.kind === "audio");
  expect(audio?.clips[0]).toMatchObject({ start: 0, end: 3, text: "hello there world", status: "draft" });
  const again = speakClip(spoken, "subs", "c1");
  expect(again.tracks.find((tr) => tr.kind === "audio")?.clips).toHaveLength(1);
  expect(
    speakClip(setClipText(again, "subs", "c1", "fixed"), "subs", "c1").tracks.find((tr) => tr.kind === "audio")
      ?.clips[0].text,
  ).toBe("fixed");
  expect(draftCount(again)).toBe(1);
  const empty = parseTimeline('{"tracks":[{"kind":"captions","clips":[{"id":"c","start":0,"end":1}]}]}');
  expect(speakClip(empty, "subs", "c")).toBe(empty);
});

test("setClipText marks a spoken audio clip draft", () => {
  const t = setClipText(parseTimeline(SAMPLE), "voice", "s1", "changed");
  expect(t.tracks[1].clips[0]).toMatchObject({ text: "changed", status: "draft" });
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

test("splitClip cuts a clip into two adjacent halves, advancing the second's offset", () => {
  const t = parseTimeline(SAMPLE);
  const s = splitClip(t, "video", "v1", 8)!;
  expect(s.tracks[0].clips.map((c) => [c.id, c.start, c.end, c.offset])).toEqual([
    ["v1", 0, 8, undefined],
    ["v2", 8, 20, 8],
  ]);
  expect(s.duration).toBe(20);
  expect(splitClip(t, "video", "v1", 8)!.tracks[0].clips[0].id).toBe("v1");
  const trimmed = splitClip(s, "video", "v2", 12)!;
  expect(trimmed.tracks[0].clips.map((c) => [c.id, c.start, c.end, c.offset])).toEqual([
    ["v1", 0, 8, undefined],
    ["v2", 8, 12, 8],
    ["v3", 12, 20, 12],
  ]);
  expect(splitClip(trimmed, "video", "v3", 13)!.tracks[0].clips[3].offset).toBe(13);
  expect(splitClip(t, "video", "v1", 0.1)).toBeNull();
  expect(splitClip(t, "video", "v1", 19.9)).toBeNull();
  expect(splitClip(t, "video", "zz", 5)).toBeNull();
});

test("splitClip keeps a split spoken clip editable and marks the second half draft", () => {
  const t = parseTimeline(SAMPLE);
  const s = splitClip(t, "voice", "s1", 5)!;
  expect(s.tracks[1].clips.map((c) => [c.id, c.start, c.end, c.status])).toEqual([
    ["s1", 0, 5, "done"],
    ["s3", 5, 10, "draft"],
    ["s2", 10, 20, "done"],
  ]);
  expect(s.tracks[1].clips[0].text).toBe("first");
});

test("splitClip and trimClip round-trip offset through parse/serialize", () => {
  const t = parseTimeline(SAMPLE);
  const s = splitClip(t, "video", "v1", 6)!;
  const head = trimClip(s, "video", "v1", "start", 2);
  const round = parseTimeline(serializeTimeline(head));
  expect(round.tracks[0].clips[0]).toMatchObject({ id: "v1", start: 2, end: 6, offset: 2 });
  expect(round.tracks[0].clips[1]).toMatchObject({ id: "v2", start: 6, end: 20, offset: 6 });
});

test("snapTime picks the nearest point within tolerance", () => {
  const t = addClip(addClip(emptyTimeline(), "audio", "a.mp3", 4, 0), "audio", "b.mp3", 6, 6);
  expect(snapPoints(t, "a2", 9).sort((a, b) => a - b)).toEqual([0, 0, 4, 9]);
  expect(snapTime(3.9, [0, 4, 9], 0.2)).toBe(4);
  expect(snapTime(3.5, [0, 4, 9], 0.2)).toBe(3.5);
  expect(snapTime(4.1, [4, 4.15], 0.2)).toBe(4.15);
});

describe("clipSample", () => {
  test("falls back to the track's pick and colours every sample apart", () => {
    const t = parseTimeline(
      JSON.stringify({
        tracks: [
          {
            id: "voice",
            kind: "audio",
            voice_sample: "eden.wav",
            clips: [
              { id: "s1", start: 0, end: 5, text: "own", voice_sample: "ada.wav" },
              { id: "s2", start: 5, end: 10, text: "track" },
              { id: "m1", start: 10, end: 15, src: "media/music.mp3" },
            ],
          },
        ],
      }),
    );
    const [own, inherited, music] = t.tracks[0].clips;
    expect(clipSample(t.tracks[0], own)).toBe("ada.wav");
    expect(clipSample(t.tracks[0], inherited)).toBe("eden.wav");
    expect(clipSample(t.tracks[0], music)).toBeUndefined();
    expect(sampleColour("ada.wav")).toBe(sampleColour("ada.wav"));
    expect(sampleColour("ada.wav")).not.toBe(sampleColour("eden.wav"));
  });

  test("picking another voice drafts the clip, picking the same one leaves it alone", () => {
    const t = parseTimeline(
      JSON.stringify({
        tracks: [
          {
            id: "voice",
            kind: "audio",
            voice_sample: "eden.wav",
            clips: [{ id: "s1", start: 0, end: 5, text: "hi", src: "media/s1.wav", status: "done" }],
          },
        ],
      }),
    );
    const picked = setClipSample(t, "voice", "s1", "ada.wav").tracks[0].clips[0];
    expect(picked.voice_sample).toBe("ada.wav");
    expect(picked.status).toBe("draft");
    expect(setClipSample(t, "voice", "s1", "eden.wav").tracks[0].clips[0].status).toBe("done");
    expect(setClipSample(t, "voice", "s1", undefined).tracks[0].clips[0].status).toBe("done");
  });
});

describe("keyframes", () => {
  const timelineOf = (clip: Clip): Timeline => ({
    version: 1,
    duration: clip.end,
    tracks: [{ id: "video", kind: "video", clips: [clip] }],
  });

  test("sampleKf holds before the first key and after the last, lerps between", () => {
    const ch = [
      { t: 1, v: 10 },
      { t: 3, v: 20 },
    ];
    expect(sampleKf(ch, 0, 99)).toBe(10);
    expect(sampleKf(ch, 2, 99)).toBe(15);
    expect(sampleKf(ch, 5, 99)).toBe(20);
    expect(sampleKf(undefined, 2, 42)).toBe(42);
    expect(sampleKf([], 2, 42)).toBe(42);
  });

  test("framingAt samples animated fields and passes static ones through", () => {
    const clip: Clip = {
      id: "v1",
      start: 2,
      end: 6,
      x: 0.3,
      scale: 1,
      keys: {
        scale: [
          { t: 0, v: 1 },
          { t: 2, v: 2 },
        ],
      },
    };
    expect(framingAt(clip, 3).scale).toBe(1.5);
    expect(framingAt(clip, 3).x).toBe(0.3);
    expect(framingAt({ id: "v", start: 0, end: 1, x: 0.4, scale: 2 }, 0.5)).toEqual({ x: 0.4, y: undefined, scale: 2 });
  });

  test("sourceConsumed is linear for constant speed", () => {
    expect(sourceConsumed({ id: "v", start: 0, end: 4, speed: 0.5 }, 4)).toBe(2);
    expect(sourceConsumed({ id: "v", start: 0, end: 4 }, 3)).toBe(3);
    expect(sourceConsumed({ id: "v", start: 0, end: 4, speed: 2 }, 1)).toBe(2);
    expect(sourceTimeAt({ id: "v", start: 5, end: 9, offset: 2, speed: 2 }, 7)).toBe(6);
  });

  test("an ease-in-out clip bells 1x at the edges to the target in the middle", () => {
    const eased: Clip = { id: "v", start: 0, end: 4, speed: 3, speed_ease: "easeInOut" };
    expect(speedAt(eased, 0)).toBeCloseTo(1, 6);
    expect(speedAt(eased, 4)).toBeCloseTo(1, 6);
    expect(speedAt(eased, 2)).toBeCloseTo(3, 6);

    expect(sourceConsumed(eased, 4)).toBeCloseTo(8, 6);
    expect(sourceConsumed(eased, 2)).toBeCloseTo(4, 6);
    expect(avgSpeed(eased)).toBeCloseTo(2, 6);
    expect(avgSpeed({ id: "v", start: 0, end: 4, speed: 3 })).toBeCloseTo(3, 6);
  });

  test("setKf upserts, toggleKf captures then removes, moveKf retimes a diamond", () => {
    const on = toggleKf(timelineOf({ id: "v1", start: 0, end: 10, scale: 1.2 }), "v1", "scale", 3);
    expect(on.tracks[0].clips[0].keys?.scale).toEqual([{ t: 3, v: 1.2 }]);
    const set = setKf(setKf(on, "v1", "scale", 3, 2), "v1", "scale", 6, 3);
    expect(set.tracks[0].clips[0].keys?.scale).toEqual([
      { t: 3, v: 2 },
      { t: 6, v: 3 },
    ]);
    const moved = moveKf(set, "v1", 3, 4);
    expect(moved.tracks[0].clips[0].keys?.scale).toEqual([
      { t: 4, v: 2 },
      { t: 6, v: 3 },
    ]);
    expect(toggleKf(moved, "v1", "scale", 6).tracks[0].clips[0].keys?.scale).toEqual([{ t: 4, v: 2 }]);
  });

  test("splitClip advances the second offset by the source consumed and carries speed to both halves", () => {
    const base = timelineOf({ id: "v1", src: "a.mov", start: 0, end: 4, offset: 0, speed: 2, speed_ease: "easeInOut" });
    const clips = splitClip(base, "video", "v1", 2)!.tracks[0].clips;
    expect(clips).toHaveLength(2);
    expect(clips[1].offset).toBeCloseTo(3, 6);
    expect(clips[0]).toMatchObject({ speed: 2, speed_ease: "easeInOut" });
    expect(clips[1]).toMatchObject({ speed: 2, speed_ease: "easeInOut" });
  });

  test("trimClip head advances the offset by the source consumed at speed", () => {
    const base = timelineOf({ id: "v1", src: "a.mov", start: 0, end: 4, offset: 1, speed: 2 });
    const out = trimClip(base, "video", "v1", "start", 1);
    expect(out.tracks[0].clips[0]).toMatchObject({ start: 1 });
    expect(out.tracks[0].clips[0].offset).toBeCloseTo(3, 6);
  });

  test("keyframes and speed survive a parse round-trip, sorted and clamped", () => {
    const json = serializeTimeline(
      timelineOf({
        id: "v1",
        start: 0,
        end: 5,
        speed: 99,
        speed_ease: "easeInOut",
        keys: {
          scale: [
            { t: 3, v: 2 },
            { t: 1, v: 1 },
          ],
        },
      }),
    );
    const clip = parseTimeline(json).tracks[0].clips[0];
    expect(clip.keys?.scale).toEqual([
      { t: 1, v: 1 },
      { t: 3, v: 2 },
    ]);
    expect(clip.speed).toBe(MAX_SPEED);
    expect(clip.speed_ease).toBe("easeInOut");
  });

  test("setSpeed resizes the clip, holding the source span, and stops at the next clip", () => {
    const base = timelineOf({ id: "v1", src: "a.mov", start: 0, end: 4 });
    expect(setSpeed(base, "v1", 2).tracks[0].clips[0]).toMatchObject({ speed: 2, end: 2 });
    expect(setSpeed(base, "v1", 99).tracks[0].clips[0].speed).toBe(MAX_SPEED);

    const eased = setSpeed(base, "v1", 2, "easeInOut").tracks[0].clips[0];
    expect(eased).toMatchObject({ speed: 2, speed_ease: "easeInOut" });
    expect(eased.end).toBeCloseTo(4 / 1.5, 6);

    expect(setSpeed(setSpeed(base, "v1", 2, "easeInOut"), "v1", 4).tracks[0].clips[0].speed_ease).toBe("easeInOut");

    expect(setSpeed(setSpeed(base, "v1", 3), "v1", 1).tracks[0].clips[0].end).toBeCloseTo(4, 6);
    const two: Timeline = {
      version: 1,
      duration: 10,
      tracks: [
        {
          id: "video",
          kind: "video",
          clips: [
            { id: "v1", src: "a.mov", start: 0, end: 4 },
            { id: "v2", src: "b.mov", start: 5, end: 8 },
          ],
        },
      ],
    };
    expect(setSpeed(two, "v1", 0.5).tracks[0].clips[0].end).toBeCloseTo(5, 6);
  });

  test("clearKeys removes one property or all, removeKf deletes a diamond", () => {
    const t = setKf(setKf(timelineOf({ id: "v1", start: 0, end: 10 }), "v1", "scale", 2, 1.5), "v1", "x", 2, 0.3);
    expect(clearKeys(t, "v1", "scale").tracks[0].clips[0].keys).toEqual({ x: [{ t: 2, v: 0.3 }] });
    expect(clearKeys(t, "v1").tracks[0].clips[0].keys).toBeUndefined();
    expect(removeKf(t, "v1", 2).tracks[0].clips[0].keys).toBeUndefined();
  });
});
