// The one renderer. It composes a timeline frame onto a 2D canvas - video,
// overlay cards, captions - and both paths use it: the preview draws it live
// into the stage, the export draws it at the frame's full pixel size and hands
// the pixels to ffmpeg. There is no second implementation to keep in sync,
// which is the whole point: a look is written once and the export gets it.
//
// The frame is `resolution` and the clip is scaled to cover it, so a clip of a
// different shape is cropped. `drawPreview` shows the whole clip with the part
// that falls outside the frame blurred and dimmed, so the crop is visible
// before it is exported.
//
// ponytail: Canvas2D, not WebGPU/WebGL2. WKWebView does not reliably expose
// navigator.gpu, and everything here (drawImage, roundRect, strokeText,
// ctx.filter) is native. Upgrade path if an effect needs per-pixel work: back
// drawFrame with a WebGL2 context behind the same signature.
import {
  captionPreset,
  captionStyle,
  frameDims,
  type CaptionStyle,
  type Clip,
  type Timeline,
  type Track,
} from "./timeline";

export type Rect = { x: number; y: number; w: number; h: number };

// Pinned so the same JSON draws the same pixels. This is the app's own stack
// (--font-sans in frontend/index.css), so the export matches what the user was
// editing against rather than the Arial the old ASS export hard-coded.
export const FONT_STACK = '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif';
// Fraction of the frame a caption block may span, and its margin off the edge.
const CAPTION_MAX_W = 0.9;
const CAPTION_MARGIN = 0.06;
const LINE_HEIGHT = 1.25;
// Band padding and corner radius, as fractions of the font size.
const BAND_PAD_X = 0.5;
const BAND_PAD_Y = 0.2;
const BAND_RADIUS = 0.15;
// Width the blurred backdrop is downscaled to before being scaled back up:
// the upscale is the blur, which costs nothing next to blurring at full size.
const BLUR_W = 96;
const BLUR_PX = 6;
const WING_DIM = 0.45;

// What a clip draws from, resolved by the caller (a <video>, usually).
export type Sources = (clipId: string) => CanvasImageSource | null;
// Measures `text` at `font`; the caller supplies a canvas, a test a stub.
export type Measure = (text: string, font: string) => number;

function sizeOf(img: CanvasImageSource): { w: number; h: number } | null {
  const any = img as {
    videoWidth?: number;
    videoHeight?: number;
    naturalWidth?: number;
    width?: number;
    height?: number;
  };
  const w = any.videoWidth || any.naturalWidth || (typeof any.width === "number" ? any.width : 0);
  const h = any.videoHeight || (typeof any.height === "number" ? any.height : 0);
  return w > 0 && h > 0 ? { w, h } : null;
}

// Largest sw:sh rect that fits inside dw x dh, centred.
export function containRect(sw: number, sh: number, dw: number, dh: number): Rect {
  const scale = Math.min(dw / sw, dh / sh);
  const w = sw * scale;
  const h = sh * scale;
  return { x: (dw - w) / 2, y: (dh - h) / 2, w, h };
}

// Smallest sw:sh rect that covers dw x dh, centred - the crop the export takes.
export function coverRect(sw: number, sh: number, dw: number, dh: number): Rect {
  const scale = Math.max(dw / sw, dh / sh);
  const w = sw * scale;
  const h = sh * scale;
  return { x: (dw - w) / 2, y: (dh - h) / 2, w, h };
}

const visible = (t: Timeline, kind: Track["kind"], hidden?: ReadonlySet<string>) =>
  t.tracks.filter((tr) => tr.kind === kind && !hidden?.has(tr.id));

// The shape the stage is laid out to: the first video clip whose metadata has
// loaded, not whichever clip is showing. Taking it from the playhead would
// resize the frame rect under the user every time they scrubbed across a gap.
function stageSize(t: Timeline, sources: Sources, hidden?: ReadonlySet<string>): { w: number; h: number } | null {
  const clips = visible(t, "video", hidden)
    .flatMap((tr) => tr.clips)
    .filter((c) => c.src)
    .sort((a, b) => a.start - b.start);
  for (const c of clips) {
    const img = sources(c.id);
    const size = img && sizeOf(img);
    if (size) return size;
  }
  return null;
}

// The video clip showing at `now`, or nothing in a gap between clips. A gap
// is black, which is what the lanes show and what the preview has always
// drawn; the old export froze the previous frame there instead, and the two
// disagreeing is the kind of drift this renderer exists to end.
export function activeVideo(t: Timeline, now: number, hidden?: ReadonlySet<string>): Clip | null {
  return (
    visible(t, "video", hidden)
      .flatMap((tr) => tr.clips)
      .find((c) => c.src && now >= c.start && now < c.end) ?? null
  );
}

// An overlay card's box in frame pixels, keeping the rule the CSS preview and
// the ffmpeg overlay filter both had: `width` alone keeps the aspect, `height`
// alone keeps the aspect, neither means the full frame width.
export function overlayRect(c: Clip, src: { w: number; h: number }, fw: number, fh: number): Rect {
  const w = c.width !== undefined ? c.width * fw : c.height !== undefined ? (c.height * fh * src.w) / src.h : fw;
  const h = c.height !== undefined ? c.height * fh : (w * src.h) / src.w;
  return { x: (c.x ?? 0) * fw, y: (c.y ?? 0) * fh, w, h };
}

export type CaptionLine = { tokens: { text: string; x: number; w: number; index: number }[]; w: number };
export type CaptionLayout = {
  box: Rect;
  lines: CaptionLine[];
  font: string;
  fontPx: number;
  preset: CaptionStyle;
  words: Clip["words"];
};

// Where a caption sits and how it wraps, shared by the renderer and by the
// invisible drag handle the stage puts over it.
export function layoutCaption(
  track: Track,
  clip: Clip,
  fw: number,
  fh: number,
  measure: Measure,
): CaptionLayout | null {
  const preset = captionPreset(captionStyle(track.style));
  const words = preset.words && clip.words?.length ? clip.words : undefined;
  const raw = clip.text?.split(/\s+/).filter(Boolean) ?? [];
  const tokens = (words ? words.map((w, i) => w.text ?? raw[i] ?? "") : raw).map((s) =>
    preset.upper ? s.toUpperCase() : s,
  );
  if (!tokens.length) return null;
  const fontPx = preset.size * fh;
  const font = `${preset.weight} ${fontPx}px ${FONT_STACK}`;
  const space = measure(" ", font);
  const maxW = CAPTION_MAX_W * fw;
  const lines: CaptionLine[] = [];
  let line: CaptionLine = { tokens: [], w: 0 };
  tokens.forEach((text, index) => {
    const w = measure(text, font);
    const advance = line.tokens.length ? space + w : w;
    if (line.tokens.length && line.w + advance > maxW) {
      lines.push(line);
      line = { tokens: [], w: 0 };
    }
    const x = line.tokens.length ? line.w + space : 0;
    line.tokens.push({ text, x, w, index });
    line.w = x + w;
  });
  lines.push(line);
  const padX = preset.band ? BAND_PAD_X * fontPx : 0;
  const padY = preset.band ? BAND_PAD_Y * fontPx : 0;
  const boxW = Math.max(...lines.map((l) => l.w)) + padX * 2;
  const boxH = lines.length * fontPx * LINE_HEIGHT + padY * 2;
  const cx = track.x !== undefined ? track.x * fw : fw / 2;
  const margin = CAPTION_MARGIN * fh;
  const cy =
    track.y !== undefined
      ? track.y * fh
      : track.position === "top"
        ? margin + boxH / 2
        : track.position === "center"
          ? fh / 2
          : fh - margin - boxH / 2;
  return {
    box: { x: cx - boxW / 2, y: cy - boxH / 2, w: boxW, h: boxH },
    lines,
    font,
    fontPx,
    preset,
    words,
  };
}

function drawCaption(ctx: CanvasRenderingContext2D, l: CaptionLayout, now: number): void {
  const { preset, fontPx, box, lines } = l;
  if (preset.band) {
    ctx.fillStyle = "rgba(0,0,0,0.6)";
    ctx.beginPath();
    ctx.roundRect(box.x, box.y, box.w, box.h, BAND_RADIUS * fontPx);
    ctx.fill();
  }
  ctx.font = l.font;
  ctx.textBaseline = "middle";
  ctx.textAlign = "left";
  ctx.lineJoin = "round";
  ctx.miterLimit = 2;
  ctx.strokeStyle = "#000000";
  ctx.lineWidth = preset.outline * fontPx * 2;
  const padY = preset.band ? BAND_PAD_Y * fontPx : 0;
  lines.forEach((line, row) => {
    const left = box.x + (box.w - line.w) / 2;
    const y = box.y + padY + (row + 0.5) * fontPx * LINE_HEIGHT;
    for (const tok of line.tokens) {
      const word = l.words?.[tok.index];
      ctx.fillStyle = !l.words
        ? preset.colour
        : now >= (word?.start ?? 0)
          ? preset.colour
          : (preset.ahead ?? preset.colour);
      if (preset.outline) ctx.strokeText(tok.text, left + tok.x, y);
      ctx.fillText(tok.text, left + tok.x, y);
    }
  });
}

// Compose the frame into the current transform, which the caller has set so
// that (0,0)-(fw,fh) is the export frame. `hidden` is a view option only - the
// preview passes the lanes the user hid, the export passes nothing.
export function drawFrame(
  ctx: CanvasRenderingContext2D,
  t: Timeline,
  now: number,
  sources: Sources,
  hidden?: ReadonlySet<string>,
): void {
  const [fw, fh] = frameDims(t);
  ctx.fillStyle = "#000000";
  ctx.fillRect(0, 0, fw, fh);
  const base = activeVideo(t, now, hidden);
  const img = base && sources(base.id);
  const size = img && sizeOf(img);
  if (img && size) {
    const r = coverRect(size.w, size.h, fw, fh);
    ctx.drawImage(img, r.x, r.y, r.w, r.h);
  }
  for (const tr of visible(t, "overlay", hidden)) {
    for (const c of tr.clips) {
      if (!c.src || now < c.start || now >= c.end) continue;
      const card = sources(c.id);
      const cardSize = card && sizeOf(card);
      if (!card || !cardSize) continue;
      const r = overlayRect(c, cardSize, fw, fh);
      ctx.drawImage(card, r.x, r.y, r.w, r.h);
    }
  }
  const captions = visible(t, "captions", hidden)[0];
  const clip = captions?.clips.find((c) => now >= c.start && now < c.end);
  if (!captions || !clip) return;
  const layout = layoutCaption(captions, clip, fw, fh, (text, font) => {
    ctx.font = font;
    return ctx.measureText(text).width;
  });
  if (layout) drawCaption(ctx, layout, now);
}

let scratch: HTMLCanvasElement | null = null;

// The stage: the whole clip, with the export frame sharp and everything
// outside it blurred and dimmed. Returns the frame's rect in canvas pixels so
// the caller can hit-test the caption against it.
export function drawPreview(
  ctx: CanvasRenderingContext2D,
  t: Timeline,
  now: number,
  sources: Sources,
  w: number,
  h: number,
  hidden?: ReadonlySet<string>,
): Rect {
  const [fw, fh] = frameDims(t);
  ctx.clearRect(0, 0, w, h);
  const size = stageSize(t, sources, hidden);
  const srcRect = size ? containRect(size.w, size.h, w, h) : containRect(fw, fh, w, h);
  const inner = containRect(fw, fh, srcRect.w, srcRect.h);
  const frame: Rect = { x: srcRect.x + inner.x, y: srcRect.y + inner.y, w: inner.w, h: inner.h };
  const base = activeVideo(t, now, hidden);
  const img = base && sources(base.id);
  if (img && size && (inner.w < srcRect.w - 1 || inner.h < srcRect.h - 1)) {
    const small = (scratch ??= document.createElement("canvas"));
    small.width = BLUR_W;
    small.height = Math.max(1, Math.round((BLUR_W * size.h) / size.w));
    const sctx = small.getContext("2d");
    if (sctx) {
      sctx.drawImage(img, 0, 0, small.width, small.height);
      ctx.save();
      ctx.filter = `blur(${BLUR_PX}px)`;
      ctx.drawImage(small, srcRect.x, srcRect.y, srcRect.w, srcRect.h);
      ctx.restore();
      ctx.fillStyle = `rgba(0,0,0,${WING_DIM})`;
      ctx.fillRect(srcRect.x, srcRect.y, srcRect.w, srcRect.h);
    }
  }
  ctx.save();
  ctx.translate(frame.x, frame.y);
  ctx.scale(frame.w / fw, frame.h / fh);
  ctx.beginPath();
  ctx.rect(0, 0, fw, fh);
  ctx.clip();
  drawFrame(ctx, t, now, sources, hidden);
  ctx.restore();
  ctx.strokeStyle = "rgba(255,255,255,0.5)";
  ctx.lineWidth = 1;
  ctx.strokeRect(frame.x + 0.5, frame.y + 0.5, frame.w - 1, frame.h - 1);
  return frame;
}
