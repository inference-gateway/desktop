// Walking the timeline frame by frame and handing the pixels to ffmpeg. The
// frames come from the same drawFrame() the preview uses, drawn at the export
// frame's own pixel size, so the file is the preview by construction rather
// than by review. ffmpeg only mixes the audio and encodes; it is given the
// picture already composed, one raw RGBA frame every 1/fps.
import { api } from "./tauri";
import { activeVideo, drawFrame } from "./render";
import { serializeTimeline, type Timeline } from "./timeline";

// How long to wait for the seeked frame to be presented before drawing
// anyway. The wait is a frame when the window is on screen; when it is hidden
// the callbacks never fire, and an export must not hang because of that.
const PRESENT_TIMEOUT_MS = 50;

type Elements = (clipId: string) => HTMLMediaElement | null;

const delay = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

// `seeked` says the position moved, not that the new picture is drawable, so
// wait for the frame to be presented as well.
// ponytail: requestVideoFrameCallback where the webview has it, a double rAF
// otherwise; WebCodecs VideoDecoder is the upgrade if this is ever too slow.
function presented(el: HTMLVideoElement): Promise<void> {
  const rvfc = (el as HTMLVideoElement & { requestVideoFrameCallback?: (cb: () => void) => number })
    .requestVideoFrameCallback;
  return new Promise<void>((resolve) => {
    if (typeof rvfc === "function") rvfc.call(el, () => resolve());
    else requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
  });
}

function seekTo(el: HTMLMediaElement, time: number): Promise<void> {
  if (!el.paused) el.pause();
  if (Math.abs(el.currentTime - time) < 1e-6) return Promise.resolve();
  return new Promise<void>((resolve) => {
    el.addEventListener("seeked", () => resolve(), { once: true });
    el.currentTime = time;
  }).then(() =>
    el instanceof HTMLVideoElement ? Promise.race([presented(el), delay(PRESENT_TIMEOUT_MS)]) : undefined,
  );
}

// Every element the frame at `now` draws from, put on its own local time -
// the same `offset + (now - start)` the preview plays at.
async function seekAll(t: Timeline, now: number, elements: Elements): Promise<void> {
  const clips = [
    activeVideo(t, now),
    ...t.tracks
      .filter((tr) => tr.kind === "overlay")
      .flatMap((tr) => tr.clips)
      .filter((c) => c.src && now >= c.start && now < c.end),
  ];
  await Promise.all(
    clips.flatMap((c) => {
      const el = c && elements(c.id);
      return el ? [seekTo(el, (c.offset ?? 0) + (now - c.start))] : [];
    }),
  );
}

// Render the whole timeline into the ffmpeg the backend has waiting. Frame n
// is drawn at exactly n / fps, never in real time, so the same JSON produces
// the same frames. Returns the exported file name.
export async function runExport(
  project: string,
  name: string,
  timeline: Timeline,
  elements: Elements,
  onProgress: (frame: number, frames: number) => void,
): Promise<string> {
  const plan = await api.exportBegin(project, name, serializeTimeline(timeline));
  try {
    const canvas = document.createElement("canvas");
    canvas.width = plan.width;
    canvas.height = plan.height;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) throw new Error("this webview has no 2D canvas context");
    for (let n = 0; n < plan.frames; n++) {
      const now = n / plan.fps;
      await seekAll(timeline, now, elements);
      drawFrame(ctx, timeline, now, (id) => elements(id) as CanvasImageSource | null);
      await api.exportFrame(ctx.getImageData(0, 0, plan.width, plan.height).data.buffer as ArrayBuffer);
      onProgress(n + 1, plan.frames);
    }
    return await api.exportEnd();
  } catch (e) {
    await api.exportCancel().catch(() => {});
    throw e;
  }
}
