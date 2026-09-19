import { describe, expect, test } from "bun:test";
import { activeVideo, containRect, coverRect, layoutCaption, overlayRect } from "./render";
import type { Timeline, Track } from "./timeline";

// Every glyph is one unit wide per character, so a layout assertion reads as
// character counts instead of font metrics.
const measure = (text: string) => text.length;

const timeline = (tracks: Track[]): Timeline => ({ version: 1, duration: 10, tracks });

describe("fitting", () => {
  test("contain fits inside and cover fills, both centred", () => {
    expect(containRect(16, 9, 1080, 1920)).toEqual({ x: 0, y: 656.25, w: 1080, h: 607.5 });
    expect(coverRect(16, 9, 1080, 1920)).toEqual({ x: -1166.6666666666667, y: 0, w: 3413.3333333333335, h: 1920 });
  });

  test("an equal aspect neither crops nor pads", () => {
    expect(containRect(1920, 1080, 960, 540)).toEqual({ x: 0, y: 0, w: 960, h: 540 });
    expect(coverRect(1920, 1080, 960, 540)).toEqual({ x: 0, y: 0, w: 960, h: 540 });
  });

  test("the frame inscribed in the clip is the region cover exports", () => {
    // A 9:16 frame over a 16:9 clip shown 1600 wide: the sharp rect is the
    // inscribed 9:16 box, and everything either side of it is the blurred wing.
    const clip = containRect(16, 9, 1600, 900);
    const frame = containRect(1080, 1920, clip.w, clip.h);
    expect(frame.w).toBeCloseTo(506.25, 5);
    expect(frame.h).toBe(900);
    expect(frame.x).toBeCloseTo(546.875, 5);
  });
});

describe("overlay boxes", () => {
  const src = { w: 800, h: 400 };
  const at = (c: Partial<{ width: number; height: number }>) =>
    overlayRect({ id: "o1", start: 0, end: 1, ...c }, src, 1920, 1080);

  test("width alone keeps the aspect", () => {
    expect(at({ width: 0.5 })).toMatchObject({ w: 960, h: 480 });
  });

  test("height alone keeps the aspect", () => {
    expect(at({ height: 0.5 })).toMatchObject({ w: 1080, h: 540 });
  });

  test("neither means the full frame width", () => {
    expect(at({})).toMatchObject({ w: 1920, h: 960 });
  });

  test("both are taken literally, and x/y are fractions of the frame", () => {
    expect(
      overlayRect({ id: "o1", start: 0, end: 1, x: 0.25, y: 0.5, width: 0.4, height: 0.2 }, src, 1920, 1080),
    ).toEqual({ x: 480, y: 540, w: 768, h: 216 });
  });
});

describe("the active video clip", () => {
  const t = timeline([
    {
      id: "video",
      kind: "video",
      clips: [
        { id: "v1", start: 0, end: 2, src: "a.mp4" },
        { id: "v2", start: 5, end: 8, src: "b.mp4" },
      ],
    },
  ]);

  test("a clip covers its own range, half open at the end", () => {
    expect(activeVideo(t, 0)?.id).toBe("v1");
    expect(activeVideo(t, 1.999)?.id).toBe("v1");
    expect(activeVideo(t, 2)).toBeNull();
    expect(activeVideo(t, 5)?.id).toBe("v2");
  });

  test("a gap is black, not the previous frame held", () => {
    expect(activeVideo(t, 3)).toBeNull();
    expect(activeVideo(t, 9)).toBeNull();
  });

  test("a hidden lane draws nothing", () => {
    expect(activeVideo(t, 1, new Set(["video"]))).toBeNull();
  });
});

describe("caption layout", () => {
  const track = (extra: Partial<Track> = {}): Track => ({ id: "cap", kind: "captions", clips: [], ...extra });
  const clip = { id: "c1", start: 0, end: 2, text: "hello there" };

  test("it sits on the bottom margin by default, centred on the frame", () => {
    const l = layoutCaption(track(), clip, 1920, 1080, measure)!;
    // classic: size 0.048 of 1080 = 51.84px, one line, band padding 0.2em.
    expect(l.fontPx).toBeCloseTo(51.84, 5);
    expect(l.lines).toHaveLength(1);
    expect(l.box.x + l.box.w / 2).toBeCloseTo(960, 5);
    expect(l.box.y + l.box.h).toBeCloseTo(1080 - 0.06 * 1080, 5);
  });

  test("position moves the block, and a dragged x/y wins over it", () => {
    const top = layoutCaption(track({ position: "top" }), clip, 1920, 1080, measure)!;
    expect(top.box.y).toBeCloseTo(0.06 * 1080, 5);
    const dragged = layoutCaption(track({ position: "top", x: 0.25, y: 0.5 }), clip, 1920, 1080, measure)!;
    expect(dragged.box.x + dragged.box.w / 2).toBeCloseTo(480, 5);
    expect(dragged.box.y + dragged.box.h / 2).toBeCloseTo(540, 5);
  });

  test("it wraps at 90% of the frame and uppercases when the preset says so", () => {
    const long = { ...clip, text: "a ".repeat(40).trim() };
    // "bold" is upper-case, and measure() counts characters, so 90% of a
    // 20-wide frame fits 18 characters - nine "a " pairs - per line.
    const l = layoutCaption(track({ style: "bold" }), long, 20, 1080, measure)!;
    expect(l.lines.length).toBeGreaterThan(1);
    expect(l.lines[0].w).toBeLessThanOrEqual(18);
    expect(l.lines[0].tokens[0].text).toBe("A");
  });

  test("word timings are carried through for the per-word presets", () => {
    const words = [
      { text: "hello", start: 0, end: 1 },
      { text: "there", start: 1, end: 2 },
    ];
    expect(layoutCaption(track({ style: "karaoke" }), { ...clip, words }, 1920, 1080, measure)!.words).toEqual(words);
    // A preset that does not do per-word colour ignores the timings.
    expect(layoutCaption(track({ style: "classic" }), { ...clip, words }, 1920, 1080, measure)!.words).toBeUndefined();
  });

  test("an empty caption lays out nothing", () => {
    expect(layoutCaption(track(), { id: "c1", start: 0, end: 1 }, 1920, 1080, measure)).toBeNull();
  });
});
