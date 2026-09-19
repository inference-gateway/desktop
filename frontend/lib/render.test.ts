import { describe, expect, test } from "bun:test";
import { activeVideo, containRect, coverRect, fitScale, layoutCaption, overlayRect, videoRect } from "./render";
import { framingAt, type Clip, type Timeline, type Track } from "./timeline";

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

describe("video framing", () => {
  const src = { w: 1280, h: 720 };
  const clip = (extra: { x?: number; y?: number; scale?: number } = {}) => ({ id: "v1", start: 0, end: 1, ...extra });

  test("an untouched clip covers the frame, cropped and centred", () => {
    expect(videoRect(clip(), src, 1080, 1920)).toEqual(coverRect(src.w, src.h, 1080, 1920));
  });

  test("the fit scale is exactly the scale at which the whole clip is visible", () => {
    const r = videoRect(clip({ scale: fitScale(src, 1080, 1920) }), src, 1080, 1920);
    expect(r.w).toBeCloseTo(1080, 5);
    expect(r.h).toBeCloseTo(607.5, 5);
    // A square frame over a 16:9 clip has to come down to 9/16 to fit.
    expect(fitScale({ w: 16, h: 9 }, 100, 100)).toBeCloseTo(9 / 16, 10);
    // Fill is scale 1, so fitting never scales up.
    expect(fitScale(src, 1920, 1080)).toBe(1);
  });

  test("x and y are the point in the frame the clip is centred on", () => {
    const filled = videoRect(clip(), src, 1000, 1000);
    const moved = videoRect(clip({ x: 0.25, y: 0.75 }), src, 1000, 1000);
    expect(moved.x - filled.x).toBeCloseTo(-250, 5);
    expect(moved.y - filled.y).toBeCloseTo(250, 5);
    expect(moved.w).toBe(filled.w);
  });

  test("a zero or missing scale falls back to filling", () => {
    expect(videoRect(clip({ scale: 0 }), src, 1000, 1000)).toEqual(videoRect(clip(), src, 1000, 1000));
  });

  test("a scale keyframe channel drives videoRect through framingAt, as the renderer wires it", () => {
    const c: Clip = {
      id: "v1",
      start: 0,
      end: 2,
      keys: {
        scale: [
          { t: 0, v: 1 },
          { t: 2, v: 2 },
        ],
      },
    };
    const mid = videoRect({ ...c, ...framingAt(c, 1) }, src, 1000, 1000); // local 1 → scale 1.5
    const filled = videoRect(clip(), src, 1000, 1000);
    expect(mid.w).toBeCloseTo(filled.w * 1.5, 5);
    expect(mid.h).toBeCloseTo(filled.h * 1.5, 5);
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
    expect(layoutCaption(track({ style: "classic" }), { ...clip, words }, 1920, 1080, measure)!.words).toBeUndefined();
  });

  test("an empty caption lays out nothing", () => {
    expect(layoutCaption(track(), { id: "c1", start: 0, end: 1 }, 1920, 1080, measure)).toBeNull();
  });
});
