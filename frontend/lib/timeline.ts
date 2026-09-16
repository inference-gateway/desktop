// The <stem>.timeline.json contract shared with the video-editing skill:
// a duration plus video and audio tracks of clips. An audio clip with
// `text` is spoken by the agent (cloned voice); one with only `src` is a
// plain file the user placed. "voice" is accepted as a legacy track kind.
export type ClipStatus = "draft" | "done";
export type TrackKind = "video" | "audio";
// What to do with the recording's own audio track: transcribe it and replace
// it with the cloned voice, drop it, or mix it under the voice.
export type SourceAudio = "transcribe" | "mute" | "keep";
export const SOURCE_AUDIO: { value: SourceAudio; label: string }[] = [
  { value: "transcribe", label: "Redo my voice: write down what I say, clean it up, clone my voice" },
  { value: "mute", label: "Mute" },
  { value: "keep", label: "Keep it under my voice" },
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
};

export type Track = {
  id: string;
  kind: TrackKind;
  clips: Clip[];
  voice_sample?: string;
  gain?: number;
};

export type Timeline = {
  version: number;
  duration: number;
  output?: string;
  source_audio?: SourceAudio;
  tracks: Track[];
};

const KINDS: TrackKind[] = ["video", "audio"];
const MARKER_SECONDS = 5;

function num(v: unknown, fallback = 0): number {
  const n = typeof v === "string" ? parseFloat(v) : (v as number);
  return Number.isFinite(n) ? n : fallback;
}

export function parseTimeline(json: string): Timeline {
  const raw = JSON.parse(json) as Partial<Timeline> | null;
  if (!raw || !Array.isArray(raw.tracks)) throw new Error("timeline has no tracks");
  const tracks: Track[] = raw.tracks.map((t, i) => ({
    id: typeof t?.id === "string" && t.id ? t.id : `track${i + 1}`,
    kind: KINDS.includes(t?.kind as TrackKind) ? (t.kind as TrackKind) : "audio",
    voice_sample: typeof t?.voice_sample === "string" ? t.voice_sample : undefined,
    gain: t?.gain === undefined ? undefined : num(t.gain, 1),
    clips: (Array.isArray(t?.clips) ? t.clips : [])
      .map((c, j) => ({
        id: typeof c?.id === "string" && c.id ? c.id : `${t?.id ?? "clip"}-${j + 1}`,
        start: num(c?.start),
        end: num(c?.end),
        offset: c?.offset === undefined ? undefined : num(c.offset),
        src: typeof c?.src === "string" ? c.src : undefined,
        text: typeof c?.text === "string" ? c.text : undefined,
        status: (c?.status === "done" ? "done" : c?.status === "draft" ? "draft" : undefined) as ClipStatus | undefined,
      }))
      .sort((a, b) => a.start - b.start),
  }));
  const clipEnd = Math.max(0, ...tracks.flatMap((t) => t.clips.map((c) => c.end)));
  return {
    version: num(raw.version, 1),
    duration: num(raw.duration, clipEnd) || clipEnd,
    output: typeof raw.output === "string" ? raw.output : undefined,
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
  return t.tracks.flatMap((tr) => tr.clips).filter(isSpoken).length;
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

export function setClipText(t: Timeline, trackId: string, clipId: string, text: string): Timeline {
  return {
    ...t,
    tracks: t.tracks.map((tr) =>
      tr.id !== trackId
        ? tr
        : { ...tr, clips: tr.clips.map((c) => (c.id !== clipId ? c : { ...c, text, status: "draft" as const })) },
    ),
  };
}

export function removeClip(t: Timeline, trackId: string, clipId: string): Timeline {
  return {
    ...t,
    tracks: t.tracks.map((tr) => (tr.id !== trackId ? tr : { ...tr, clips: tr.clips.filter((c) => c.id !== clipId) })),
  };
}

// Insert a draft spoken clip at `at`, ending at the next clip or after
// MARKER_SECONDS, whichever comes first. Creates an audio track if missing.
export function addMarker(t: Timeline, at: number, text = ""): Timeline {
  const start = Math.max(0, Math.min(at, t.duration));
  const existing = spokenTrack(t);
  const track = existing ?? { id: "audio", kind: "audio" as const, clips: [] };
  const next = track.clips.find((c) => c.start > start);
  const end = Math.min(t.duration || start + MARKER_SECONDS, next?.start ?? Infinity, start + MARKER_SECONDS);
  const clip: Clip = { id: nextId(track, "m"), start, end: Math.max(end, start + 0.5), text, status: "draft" };
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
