// The <stem>.timeline.json contract shared with the video-editing skill:
// a duration plus video, audio, overlay and captions tracks of clips. An
// audio clip with `text` is spoken by the agent (cloned voice); one with
// only `src` is a plain file the user placed. "voice" is accepted as a
// legacy track kind. An overlay clip is a rendered card (a .mov with alpha;
// the webview drops VP9 alpha in webm) composited over the video;
// `x`/`y`/`width`/`height` are fractions of the frame. A captions clip is
// text over the video, styled by the track's preset and burned in on export.
export type ClipStatus = "draft" | "done";
export type TrackKind = "video" | "audio" | "overlay" | "captions";
// Per-word timing of a caption clip (absolute seconds), for the word-by-word
// presets. Missing `text` falls back to the i-th word of the clip's text.
export type CaptionWord = { text?: string; start: number; end: number };
// What to do with the recording's own audio track: transcribe it and replace
// it with the cloned voice, drop it, or mix it under the voice.
export type SourceAudio = "transcribe" | "mute" | "keep";
// Export frame size: the standard delivery sizes. The recording is scaled to
// fit and padded; overlays are placed in this frame.
export const DEFAULT_RESOLUTION = "1920x1080";
export const RESOLUTIONS: { value: string; label: string }[] = [
  { value: "1920x1080", label: "1920x1080 Landscape" },
  { value: "1080x1920", label: "1080x1920 Portrait" },
  { value: "1350x1350", label: "1350x1350 Square" },
];
export const SOURCE_AUDIO: { value: SourceAudio; label: string }[] = [
  { value: "transcribe", label: "Replace with my cloned voice" },
  { value: "mute", label: "Mute" },
  { value: "keep", label: "Keep under the voice" },
];

export type Clip = {
  id: string;
  start: number;
  end: number;
  // Seconds into `src` where the clip begins; set by trimming a clip's head.
  offset?: number;
  src?: string;
  text?: string;
  status?: ClipStatus;
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  // The HTML composition an overlay's `src` was rendered from.
  html?: string;
  // Caption clips only: when each word is spoken, for the word presets.
  words?: CaptionWord[];
};

export type Track = {
  id: string;
  kind: TrackKind;
  clips: Clip[];
  voice_sample?: string;
  gain?: number;
  // Captions tracks only: a preset name (unknown ones fall back to the
  // default) and where the captions sit on the frame.
  style?: string;
  position?: "bottom" | "center" | "top";
};

export type Timeline = {
  version: number;
  duration: number;
  output?: string;
  resolution?: string;
  source_audio?: SourceAudio;
  tracks: Track[];
};

// Caption style presets: the catalogue is mirrored by CAPTION_PRESETS in
// backend/src/timeline.rs (the ASS export), so preview and export match.
export const CAPTION_STYLES: { value: string; label: string; words?: boolean }[] = [
  { value: "classic", label: "Classic" },
  { value: "bold", label: "Bold" },
  { value: "highlight", label: "Highlight", words: true },
  { value: "karaoke", label: "Karaoke", words: true },
];
export const DEFAULT_CAPTION_STYLE = "classic";
// Unknown preset names fall back to the default preset.
export const captionStyle = (style?: string): string =>
  CAPTION_STYLES.some((s) => s.value === style) ? style! : DEFAULT_CAPTION_STYLE;

const KINDS: TrackKind[] = ["video", "audio", "overlay", "captions"];
const MARKER_SECONDS = 5;

function num(v: unknown, fallback = 0): number {
  const n = typeof v === "string" ? parseFloat(v) : (v as number);
  return Number.isFinite(n) ? n : fallback;
}

const optNum = (v: unknown): number | undefined => (v === undefined ? undefined : num(v));

export function parseTimeline(json: string): Timeline {
  const raw = JSON.parse(json) as Partial<Timeline> | null;
  if (!raw || !Array.isArray(raw.tracks)) throw new Error("timeline has no tracks");
  const tracks: Track[] = raw.tracks.map((t, i) => ({
    id: typeof t?.id === "string" && t.id ? t.id : `track${i + 1}`,
    kind: KINDS.includes(t?.kind as TrackKind) ? (t.kind as TrackKind) : "audio",
    voice_sample: typeof t?.voice_sample === "string" ? t.voice_sample : undefined,
    gain: t?.gain === undefined ? undefined : num(t.gain, 1),
    style: typeof t?.style === "string" ? t.style : undefined,
    position: (t?.position === "center" || t?.position === "top"
      ? t.position
      : t?.position === "bottom"
        ? "bottom"
        : undefined) as Track["position"],
    clips: (Array.isArray(t?.clips) ? t.clips : [])
      .map((c, j) => ({
        id: typeof c?.id === "string" && c.id ? c.id : `${t?.id ?? "clip"}-${j + 1}`,
        start: num(c?.start),
        end: num(c?.end),
        offset: optNum(c?.offset),
        src: typeof c?.src === "string" ? c.src : undefined,
        text: typeof c?.text === "string" ? c.text : undefined,
        status: (c?.status === "done" ? "done" : c?.status === "draft" ? "draft" : undefined) as ClipStatus | undefined,
        x: optNum(c?.x),
        y: optNum(c?.y),
        width: optNum(c?.width),
        height: optNum(c?.height),
        html: typeof c?.html === "string" ? c.html : undefined,
        words: (Array.isArray(c?.words) && c.words.length
          ? c.words.map((w) => ({
              text: typeof w?.text === "string" ? w.text : undefined,
              start: num(w?.start),
              end: num(w?.end),
            }))
          : undefined) as Clip["words"],
      }))
      .sort((a, b) => a.start - b.start),
  }));
  const clipEnd = Math.max(0, ...tracks.flatMap((t) => t.clips.map((c) => c.end)));
  return {
    version: num(raw.version, 1),
    duration: num(raw.duration, clipEnd) || clipEnd,
    output: typeof raw.output === "string" ? raw.output : undefined,
    resolution: typeof raw.resolution === "string" && /^\d+x\d+$/.test(raw.resolution) ? raw.resolution : undefined,
    source_audio: SOURCE_AUDIO.some((o) => o.value === raw.source_audio) ? raw.source_audio : undefined,
    tracks,
  };
}

export function serializeTimeline(t: Timeline): string {
  return JSON.stringify(t, null, 2) + "\n";
}

export function videoSource(t: Timeline): string | undefined {
  return t.tracks.find((tr) => tr.kind === "video")?.clips[0]?.src;
}

export const isSpoken = (c: Clip): boolean => c.text !== undefined;

export function spokenCount(t: Timeline): number {
  return t.tracks
    .filter((tr) => tr.kind === "audio")
    .flatMap((tr) => tr.clips)
    .filter(isSpoken).length;
}

// Width / height of the export frame.
export function frameAspect(t: { resolution?: string }): number {
  const [w, h] = (t.resolution ?? DEFAULT_RESOLUTION).split("x").map(Number);
  return w > 0 && h > 0 ? w / h : 16 / 9;
}

export function overlayCount(t: Timeline): number {
  return t.tracks.filter((tr) => tr.kind === "overlay").flatMap((tr) => tr.clips).length;
}

// Lanes as the editor stacks them: overlays and captions above the video,
// everything else in file order. The file itself is never reordered.
export function laneOrder(tracks: Track[]): Track[] {
  const above = tracks.filter((tr) => tr.kind === "overlay" || tr.kind === "captions");
  return [...above, ...tracks.filter((tr) => tr.kind !== "overlay" && tr.kind !== "captions")];
}

// The lane markers go on: the audio track that already carries speech,
// else the first audio track.
export function spokenTrack(t: Timeline): Track | undefined {
  return (
    t.tracks.find((tr) => tr.kind === "audio" && tr.clips.some(isSpoken)) ?? t.tracks.find((tr) => tr.kind === "audio")
  );
}

export function clipLayout(clip: Clip, pxPerSec: number): { left: number; width: number } {
  return { left: clip.start * pxPerSec, width: Math.max(2, (clip.end - clip.start) * pxPerSec) };
}

// The ruler labels every `step` seconds, the smallest step that keeps labels
// at least `minPx` apart at this zoom.
export function rulerStep(pxPerSec: number, minPx = 80): number {
  return [1, 2, 5, 10, 15, 30, 60, 120, 300, 600].find((s) => s * pxPerSec >= minPx) ?? 600;
}

export function resolveSrc(dir: string, src: string): string {
  return src.startsWith("/") ? src : `${dir.replace(/\/$/, "")}/${src}`;
}

function nextId(track: Track, prefix: string): string {
  const used = new Set(track.clips.map((c) => c.id));
  let n = track.clips.length + 1;
  while (used.has(`${prefix}${n}`)) n++;
  return `${prefix}${n}`;
}

// Editing text marks a spoken audio clip draft (it needs re-synthesis); a
// caption clip edit needs nothing, the export reads the text as it is.
export function setClipText(t: Timeline, trackId: string, clipId: string, text: string): Timeline {
  const spoken = t.tracks.find((tr) => tr.id === trackId)?.kind === "audio";
  return {
    ...t,
    tracks: t.tracks.map((tr) =>
      tr.id !== trackId
        ? tr
        : {
            ...tr,
            clips: tr.clips.map((c) =>
              c.id !== clipId ? c : { ...c, text, ...(spoken ? { status: "draft" as const } : {}) },
            ),
          },
    ),
  };
}

// The timeline's captions track, if it has one (one per timeline is enough).
export function captionTrack(t: Timeline): Track | undefined {
  return t.tracks.find((tr) => tr.kind === "captions");
}

// Turn a caption into speech: a draft audio clip with the same text and range
// on the spoken track, so "Redo drafts" voices it with the usual flow. An
// audio clip already covering the same range is updated in place.
export function speakClip(t: Timeline, trackId: string, clipId: string): Timeline {
  const caption = t.tracks.find((tr) => tr.id === trackId)?.clips.find((c) => c.id === clipId);
  if (!caption?.text) return t;
  const existing = spokenTrack(t);
  const track = existing ?? { id: "audio", kind: "audio" as const, clips: [] };
  const same = (c: Clip) => c.start === caption.start && c.end === caption.end;
  const clips = (
    existing?.clips.some(same)
      ? existing.clips.map((c) => (same(c) ? { ...c, text: caption.text, status: "draft" as const } : c))
      : [
          ...(existing?.clips ?? []),
          {
            id: nextId(track, "s"),
            start: caption.start,
            end: caption.end,
            text: caption.text,
            status: "draft" as const,
          },
        ]
  ).sort((a, b) => a.start - b.start);
  const tracks = existing
    ? t.tracks.map((tr) => (tr.id === track.id ? { ...tr, clips } : tr))
    : [...t.tracks, { ...track, clips }];
  return { ...t, tracks };
}

export function removeClip(t: Timeline, trackId: string, clipId: string): Timeline {
  return {
    ...t,
    tracks: t.tracks.map((tr) => (tr.id !== trackId ? tr : { ...tr, clips: tr.clips.filter((c) => c.id !== clipId) })),
  };
}

// Insert an empty clip at `at`, ending at the next clip or after
// MARKER_SECONDS, whichever comes first: a draft spoken clip on the audio
// track, or a caption on the captions track; a caption starts after any clip
// the playhead sits in, because two captions at once burn in on top of each
// other. Creates the track if missing.
export function addMarker(t: Timeline, at: number, kind: "audio" | "captions" = "audio", text = ""): Timeline {
  const captions = kind === "captions";
  const existing = captions ? captionTrack(t) : spokenTrack(t);
  const track = existing ?? { id: kind, kind, clips: [] };
  let start = Math.max(0, Math.min(at, t.duration));
  if (captions) {
    for (const c of [...track.clips].sort((a, b) => a.start - b.start)) {
      if (start >= c.start && start < c.end) start = c.end;
    }
  }
  const next = track.clips.find((c) => c.start > start);
  const end = Math.min(t.duration || start + MARKER_SECONDS, next?.start ?? Infinity, start + MARKER_SECONDS);
  const clip: Clip = captions
    ? { id: nextId(track, "c"), start, end: Math.max(end, start + 0.5), text }
    : { id: nextId(track, "m"), start, end: Math.max(end, start + 0.5), text, status: "draft" };
  const clips = [...track.clips, clip].sort((a, b) => a.start - b.start);
  const tracks = existing
    ? t.tracks.map((tr) => (tr.id === track.id ? { ...tr, clips } : tr))
    : [...t.tracks, { ...track, clips }];
  return { ...t, tracks };
}

// What a content project shows before the agent writes any timeline: one
// video lane and one audio lane, both empty. More lanes come from addTrack.
export function emptyTimeline(): Timeline {
  return {
    version: 1,
    duration: 0,
    tracks: [
      { id: "video", kind: "video", clips: [] },
      { id: "audio", kind: "audio", clips: [] },
    ],
  };
}

export function addTrack(t: Timeline, kind: TrackKind): Timeline {
  const used = new Set(t.tracks.map((tr) => tr.id));
  let n = 1;
  while (used.has(n === 1 ? kind : `${kind}${n}`)) n++;
  return { ...t, tracks: [...t.tracks, { id: n === 1 ? kind : `${kind}${n}`, kind, clips: [] }] };
}

// Drop a media file onto a lane: the clip starts at `at`, or right after the
// last clip if that spot is taken, and the timeline grows to fit it.
export function addClip(t: Timeline, trackId: string, src: string, length: number, at: number): Timeline {
  const track = t.tracks.find((tr) => tr.id === trackId);
  if (!track) return t;
  const lastEnd = Math.max(0, ...track.clips.map((c) => c.end));
  const wanted = Math.max(0, at);
  const free = !track.clips.some((c) => wanted < c.end && wanted + length > c.start);
  const start = free ? wanted : lastEnd;
  const clip: Clip = { id: nextId(track, track.kind[0]), start, end: start + length, src };
  const clips = [...track.clips, clip].sort((a, b) => a.start - b.start);
  return {
    ...t,
    duration: Math.max(t.duration, clip.end),
    tracks: t.tracks.map((tr) => (tr.id === trackId ? { ...tr, clips } : tr)),
  };
}

// Drag a clip to a new start, keeping its length and its place between its
// neighbours: it slides freely inside the gap and stops at the clips on
// either side. The timeline grows if the clip moves past the end.
export function moveClip(t: Timeline, trackId: string, clipId: string, start: number): Timeline {
  const track = t.tracks.find((tr) => tr.id === trackId);
  const clip = track?.clips.find((c) => c.id === clipId);
  if (!track || !clip) return t;
  const length = clip.end - clip.start;
  const lo = Math.max(0, ...track.clips.filter((c) => c.id !== clipId && c.end <= clip.start).map((c) => c.end));
  const hi = Math.min(
    ...track.clips.filter((c) => c.id !== clipId && c.start >= clip.end).map((c) => c.start - length),
  );
  const next = Math.min(Math.max(start, lo), hi);
  const moved = { ...clip, start: next, end: next + length };
  return {
    ...t,
    duration: Math.max(t.duration, moved.end),
    tracks: t.tracks.map((tr) =>
      tr.id !== trackId ? tr : { ...tr, clips: tr.clips.map((c) => (c.id === clipId ? moved : c)) },
    ),
  };
}

export const MIN_CLIP_S = 0.25;

// Drag one edge of a clip. The head of a file clip keeps its content in place
// by moving `offset`, like an in-point; the tail cannot pass the source's end
// when `sourceLength` is known. Neither edge crosses a neighbour.
export function trimClip(
  t: Timeline,
  trackId: string,
  clipId: string,
  edge: "start" | "end",
  time: number,
  sourceLength?: number,
): Timeline {
  const track = t.tracks.find((tr) => tr.id === trackId);
  const clip = track?.clips.find((c) => c.id === clipId);
  if (!track || !clip) return t;
  const others = track.clips.filter((c) => c.id !== clipId);
  const offset = clip.offset ?? 0;
  let next: Clip;
  if (edge === "start") {
    const lo = Math.max(
      0,
      clip.src ? clip.start - offset : 0,
      ...others.filter((c) => c.end <= clip.start).map((c) => c.end),
    );
    const start = Math.min(Math.max(time, lo), clip.end - MIN_CLIP_S);
    next = { ...clip, start };
    if (clip.src) next.offset = offset + (start - clip.start);
  } else {
    const hi = Math.min(
      ...others.filter((c) => c.start >= clip.end).map((c) => c.start),
      clip.src && sourceLength !== undefined ? clip.start + sourceLength - offset : Infinity,
    );
    next = { ...clip, end: Math.max(Math.min(time, hi), clip.start + MIN_CLIP_S) };
  }
  if (next.offset !== undefined && next.offset < 1e-6) delete next.offset;
  return {
    ...t,
    duration: Math.max(t.duration, next.end),
    tracks: t.tracks.map((tr) =>
      tr.id !== trackId ? tr : { ...tr, clips: tr.clips.map((c) => (c.id === clipId ? next : c)) },
    ),
  };
}

// Cut a clip at `at` into two adjacent halves: the first keeps its id and
// in-point, the second starts at `at` with `offset` advanced so the source
// continues seamlessly. A spoken second half goes back to draft. Returns
// null when `at` would leave either half below MIN_CLIP_S.
export function splitClip(t: Timeline, trackId: string, clipId: string, at: number): Timeline | null {
  const track = t.tracks.find((tr) => tr.id === trackId);
  const clip = track?.clips.find((c) => c.id === clipId);
  if (!track || !clip || at < clip.start + MIN_CLIP_S || at > clip.end - MIN_CLIP_S) return null;
  const first: Clip = { ...clip, end: at };
  const second: Clip = {
    ...clip,
    id: nextId(track, clip.id[0]),
    start: at,
    status: clip.text === undefined ? clip.status : "draft",
  };
  if (clip.src) second.offset = (clip.offset ?? 0) + (at - clip.start);
  const clips = [...track.clips.filter((c) => c.id !== clipId), first, second].sort((a, b) => a.start - b.start);
  return {
    ...t,
    duration: Math.max(t.duration, second.end),
    tracks: t.tracks.map((tr) => (tr.id !== trackId ? tr : { ...tr, clips })),
  };
}

// Edges a dragged clip snaps to: the origin, the playhead and every other
// clip's start and end on any track.
export function snapPoints(t: Timeline, excludeClipId: string, playhead: number): number[] {
  return [
    0,
    playhead,
    ...t.tracks.flatMap((tr) => tr.clips.filter((c) => c.id !== excludeClipId).flatMap((c) => [c.start, c.end])),
  ];
}

export function snapTime(time: number, points: number[], tolerance: number): number {
  let best = time;
  let dist = tolerance;
  for (const p of points) {
    const d = Math.abs(p - time);
    if (d <= dist) {
      dist = d;
      best = p;
    }
  }
  return best;
}

export function draftCount(t: Timeline): number {
  return t.tracks.flatMap((tr) => tr.clips).filter((c) => c.status === "draft").length;
}

export function fmtTime(s: number): string {
  if (!Number.isFinite(s) || s < 0) return "0:00";
  const whole = Math.floor(s);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}
