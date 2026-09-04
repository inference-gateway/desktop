// The <stem>.timeline.json contract shared with the video-editing skill:
// a duration plus tracks of clips. The desktop edits narration text and
// markers here; the agent synthesizes draft clips and muxes the output.
export type ClipStatus = "draft" | "done";
export type TrackKind = "video" | "narration" | "audio";
// What to do with the recording's own audio track: transcribe it and replace
// it with the cloned voice, drop it, or mix it under the narration.
export type SourceAudio = "transcribe" | "mute" | "keep";
export const SOURCE_AUDIO: { value: SourceAudio; label: string }[] = [
  { value: "transcribe", label: "Re-voice: transcribe, polish the text, clone my voice from it" },
  { value: "mute", label: "Mute" },
  { value: "keep", label: "Keep under the narration" },
];

export type Clip = {
  id: string;
  start: number;
  end: number;
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

const KINDS: TrackKind[] = ["video", "narration", "audio"];
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

export function narrationTrack(t: Timeline): Track | undefined {
  return t.tracks.find((tr) => tr.kind === "narration");
}

export function clipLayout(clip: Clip, duration: number): { left: string; width: string } {
  if (duration <= 0) return { left: "0%", width: "0%" };
  const left = Math.max(0, Math.min(1, clip.start / duration));
  const width = Math.max(0.004, Math.min(1 - left, (clip.end - clip.start) / duration));
  return { left: `${left * 100}%`, width: `${width * 100}%` };
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

// Insert a draft narration clip at `at`, ending at the next clip or after
// MARKER_SECONDS, whichever comes first. Creates the narration track if missing.
export function addMarker(t: Timeline, at: number, text = ""): Timeline {
  const start = Math.max(0, Math.min(at, t.duration));
  const track = narrationTrack(t) ?? { id: "voice", kind: "narration" as const, clips: [] };
  const next = track.clips.find((c) => c.start > start);
  const end = Math.min(t.duration || start + MARKER_SECONDS, next?.start ?? Infinity, start + MARKER_SECONDS);
  const clip: Clip = { id: nextId(track, "m"), start, end: Math.max(end, start + 0.5), text, status: "draft" };
  const clips = [...track.clips, clip].sort((a, b) => a.start - b.start);
  const tracks = narrationTrack(t)
    ? t.tracks.map((tr) => (tr.id === track.id ? { ...tr, clips } : tr))
    : [...t.tracks, { ...track, clips }];
  return { ...t, tracks };
}

export function draftCount(t: Timeline): number {
  return t.tracks.flatMap((tr) => tr.clips).filter((c) => c.status === "draft").length;
}

export function fmtTime(s: number): string {
  if (!Number.isFinite(s) || s < 0) return "0:00";
  const whole = Math.floor(s);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}
