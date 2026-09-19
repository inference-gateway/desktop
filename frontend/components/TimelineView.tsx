import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Download,
  Eye,
  EyeOff,
  FilePlus,
  Film,
  Layers,
  Loader2,
  Mic,
  Music,
  Pause,
  Play,
  Plus,
  RectangleHorizontal,
  RectangleVertical,
  RefreshCw,
  Scissors,
  Sparkles,
  Square,
  Trash2,
  Type,
  Volume2,
  VolumeX,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { api, type ProjectFile, type VoiceSample } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { safeAudioSrc, safeProjectMediaSrc } from "@/lib/tools";
import {
  CAPTION_STYLES,
  DEFAULT_RESOLUTION,
  RESOLUTIONS,
  SOURCE_AUDIO,
  addClip,
  addEmptyClip,
  addTrack,
  captionPreset,
  captionStyle,
  captionTrack,
  clipSample,
  moveCaptions,
  clipLayout,
  draftCount,
  emptyTimeline,
  fmtTime,
  frameAspect,
  isSpoken,
  laneOrder,
  moveClip,
  overlayCount,
  rulerStep,
  snapPoints,
  snapTime,
  splitClip,
  trimClip,
  spokenCount,
  parseTimeline,
  removeClip,
  resolveSrc,
  sampleColour,
  serializeTimeline,
  setClipSample,
  setClipText,
  speakClip,
  videoSource,
  type Clip,
  type SourceAudio,
  type Timeline,
  type Track,
  type TrackKind,
} from "@/lib/timeline";
import { useDesktop } from "@/store";
import { AudioPlayer } from "./AudioPlayer";
import { Thumbnails, Waveform } from "./ClipMedia";
import { Button } from "@/components/ui/button";

const SAVE_DEBOUNCE_MS = 600;
const RELOAD_DEBOUNCE_MS = 200;
const SYNC_TOLERANCE_S = 0.3;

// Clip audio lives either in ~/.infer/tts (voice) or in the project dir (music).
function clipSrc(dir: string, src: string): string | null {
  const path = resolveSrc(dir, src);
  return safeAudioSrc(path) ?? safeProjectMediaSrc(path);
}
const TRACK_LABEL: Record<Track["kind"], string> = {
  video: "Video",
  audio: "Audio",
  overlay: "Overlay",
  captions: "Captions",
};
const TRACK_SWATCH: Record<Track["kind"], string> = {
  video: "bg-sky-500",
  audio: "bg-emerald-500",
  overlay: "bg-fuchsia-500",
  captions: "bg-zinc-400",
};
const TRACK_ICON: Record<Track["kind"], typeof Film> = { video: Film, audio: Music, overlay: Layers, captions: Type };
const AUDIO_ICON: Record<SourceAudio, typeof Mic> = { transcribe: Mic, mute: VolumeX, keep: Volume2 };
// The timeline's frame size, falling back when the file holds a size the toolbar does not offer.
const frameSize = (t: Timeline) =>
  RESOLUTIONS.some((r) => r.value === t.resolution) ? t.resolution : DEFAULT_RESOLUTION;
// px per second bounds for the zoom; snapping grabs within SNAP_PX of an edge.
const MIN_PPS = 2;
const MAX_PPS = 400;
const SNAP_PX = 8;
// Lane height minus the clip inset, the height clip media draws at.
const CLIP_H = 48;
const ZOOM_STEP = 1.5;
const ZOOM_BTN =
  "inline-flex h-5 min-w-5 items-center justify-center rounded text-zinc-400 hover:bg-white/10 hover:text-zinc-100";
const isEditable = (t: EventTarget | null) =>
  t instanceof HTMLElement &&
  (t.isContentEditable ||
    t instanceof HTMLInputElement ||
    t instanceof HTMLTextAreaElement ||
    t instanceof HTMLSelectElement);
const clipClass = (tr: Track, c: Clip) =>
  tr.kind === "video"
    ? "border-sky-400/60 bg-sky-700/80"
    : tr.kind === "overlay"
      ? "border-fuchsia-400/60 bg-fuchsia-700/80"
      : tr.kind === "captions"
        ? "border-zinc-300/50 bg-zinc-600/85"
        : !isSpoken(c)
          ? "border-emerald-400/60 bg-emerald-700/80"
          : c.status === "draft"
            ? "border-amber-300/70 bg-amber-600/85"
            : "border-violet-400/60 bg-violet-700/85";

// ponytail: length for a dropped file whose metadata could not be read (outside the projects root).
const FALLBACK_CLIP_S = 5;
const VIDEO_EXT = /\.(?:mp4|mov|m4v|webm)$/i;
const MEDIA_EXT = /\.(?:mp4|mov|m4v|webm|mp3|wav|m4a|aac|ogg|flac)$/i;

// Name used when the user starts layering tracks before the agent wrote any timeline.
const DEFAULT_TIMELINE = "main.timeline.json";
const fmtBytes = (n: number) =>
  n < 1024 ? `${n} B` : n < 1024 * 1024 ? `${Math.round(n / 1024)} KB` : `${(n / (1024 * 1024)).toFixed(1)} MB`;

// The voice picker the lane header (a track's default) and the clip editor
// share. The blank option means "agent picks"; it only names `value` when the
// library has no such sample - the recording, or one deleted since - so the
// pick is never dropped silently, and never listed twice.
function VoiceSelect({
  label,
  title,
  samples,
  value,
  onChange,
  className,
}: {
  label: string;
  title: string;
  samples: VoiceSample[];
  value?: string;
  onChange: (sample?: string) => void;
  className: string;
}) {
  const known = samples.some((v) => v.name === value);
  return (
    <select
      aria-label={label}
      title={title}
      value={known ? value : ""}
      onChange={(e) => onChange(e.target.value || undefined)}
      className={className}
    >
      <option value="">{value && !known ? `Voice: ${value}` : "Voice: agent picks"}</option>
      {samples.map((v) => (
        <option key={v.name} value={v.name}>
          Voice: {v.name}
        </option>
      ))}
    </select>
  );
}

function TrackIcon({ kind }: { kind: TrackKind }) {
  const Icon = TRACK_ICON[kind];
  return <Icon size={11} className="shrink-0 text-zinc-500" />;
}

// The active caption drawn over the video, from the same preset table the
// export reads: the whole line for the plain presets, words popped as spoken
// for highlight (they stay, like the \k tags the export burns), words filled
// as spoken for karaoke. Sizes are cqh of the stage, like the ASS style's
// fraction of PlayResY; the stage is a size container. Dragging the box sets
// the track's x/y, double-clicking clears them back to `position`.
function CaptionOverlay({
  track,
  clip,
  now,
  stage,
  onMove,
}: {
  track: Track;
  clip: Clip;
  now: number;
  stage: React.RefObject<HTMLDivElement | null>;
  onMove: (x?: number, y?: number) => void;
}) {
  const preset = captionPreset(captionStyle(track.style));
  const words = preset.words && clip.words?.length ? clip.words : undefined;
  const tokens = clip.text?.split(/\s+/).filter(Boolean) ?? [];
  const text = (i: number) => words?.[i]?.text ?? tokens[i] ?? "";
  const grab = useRef<{ dx: number; dy: number } | null>(null);
  const ring = preset.outline
    ? [
        `${preset.outline}em ${preset.outline}em 0 #000`,
        `-${preset.outline}em ${preset.outline}em 0 #000`,
        `${preset.outline}em -${preset.outline}em 0 #000`,
        `-${preset.outline}em -${preset.outline}em 0 #000`,
      ].join(", ")
    : undefined;
  const placed = track.x !== undefined && track.y !== undefined;
  const pos = track.position ?? "bottom";
  const begin = (e: ReactPointerEvent<HTMLSpanElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    grab.current = { dx: e.clientX - (box.left + box.width / 2), dy: e.clientY - (box.top + box.height / 2) };
    e.currentTarget.setPointerCapture(e.pointerId);
    e.preventDefault();
  };
  const drag = (e: ReactPointerEvent<HTMLSpanElement>) => {
    const g = grab.current;
    const rect = stage.current?.getBoundingClientRect();
    if (!g || !rect || !rect.width || !rect.height) return;
    onMove((e.clientX - g.dx - rect.left) / rect.width, (e.clientY - g.dy - rect.top) / rect.height);
  };
  const drop = (e: ReactPointerEvent<HTMLSpanElement>) => {
    grab.current = null;
    e.currentTarget.releasePointerCapture(e.pointerId);
  };
  return (
    <div
      className={cn("pointer-events-none absolute flex justify-center", !placed && "inset-x-0")}
      style={
        placed
          ? { left: `${track.x! * 100}%`, top: `${track.y! * 100}%`, transform: "translate(-50%, -50%)" }
          : pos === "bottom"
            ? { bottom: "6cqh" }
            : pos === "top"
              ? { top: "6cqh" }
              : { top: "50%", transform: "translateY(-50%)" }
      }
    >
      <span
        aria-label="Caption"
        title="Drag to place the captions, double-click to put them back"
        onPointerDown={begin}
        onPointerMove={drag}
        onPointerUp={drop}
        onPointerCancel={drop}
        onDoubleClick={() => onMove(undefined, undefined)}
        className="pointer-events-auto max-w-[90%] cursor-move touch-none whitespace-pre-wrap text-center"
        style={{
          fontSize: `${preset.size * 100}cqh`,
          fontWeight: preset.weight,
          lineHeight: 1.25,
          color: words ? (preset.ahead ?? preset.colour) : preset.colour,
          ...(preset.upper ? { textTransform: "uppercase" } : {}),
          ...(ring ? { textShadow: ring } : {}),
          ...(preset.band ? { background: "rgba(0,0,0,0.6)", padding: "0.2em 0.5em", borderRadius: "0.15em" } : {}),
        }}
      >
        {words
          ? words.map((w, i) => (
              <span key={i} style={now >= w.start ? { color: preset.colour } : undefined}>
                {text(i)}{" "}
              </span>
            ))
          : clip.text}
      </span>
    </div>
  );
}

function sourceAudioInstruction(mode: SourceAudio): string {
  switch (mode) {
    case "transcribe":
      return "The recording already has me talking: write down what I say, rewrite each part into cleaner text that keeps the meaning and timing, use my speech from the recording as the voice sample unless a library sample is chosen, and replace the original audio with the cloned voice.";
    case "mute":
      return "Ignore and drop the recording's own audio.";
    case "keep":
      return "Keep the recording's own audio mixed under my voice.";
  }
}

// Editable view of <stem>.timeline.json for the current content project:
// video stage, one lane per track with clips positioned by time, a media
// pool of the project's video and audio files, and an inspector for the
// selected voice clip. With no timeline yet it shows empty video and audio
// lanes; the user layers tracks or asks the agent to arrange the media.
// Edits mark clips draft and are debounced to disk.
export function TimelineView() {
  const { currentProject: project, promptProject, runningIds, setError } = useDesktop();
  const [dir, setDir] = useState("");
  const [names, setNames] = useState<string[]>([]);
  const [name, setName] = useState("");
  const [timeline, setTimeline] = useState<Timeline | null>(null);
  const [loadError, setLoadError] = useState("");
  const [selected, setSelected] = useState<{ track: string; clip: string } | null>(null);
  const [exporting, setExporting] = useState(false);
  const [time, setTime] = useState(0);
  const [media, setMedia] = useState<ProjectFile[]>([]);
  const [samples, setSamples] = useState<VoiceSample[]>([]);
  const [durations, setDurations] = useState<Record<string, number>>({});
  const dirtyRef = useRef(false);
  const dragClipRef = useRef<{ kind: "move" | "start" | "end"; track: string; clip: Clip; x0: number } | null>(null);
  const scrubRef = useRef(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [laneWidth, setLaneWidth] = useState(0);
  const [zoom, setZoom] = useState<number | null>(null);
  const dragRef = useRef<string | null>(null);
  const [dropLane, setDropLane] = useState<string | null>(null);
  const [importing, setImporting] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [hiddenLanes, setHiddenLanes] = useState<Set<string>>(new Set());
  const [poolOver, setPoolOver] = useState(false);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const timeRef = useRef(0);
  const setTimeAt = (t: number) => {
    timeRef.current = t;
    setTime(t);
  };
  const mediaRefs = useRef(new Map<string, HTMLMediaElement>());
  const stageRef = useRef<HTMLDivElement>(null);
  const { setStatus } = useDesktop();

  const syncMedia = (t: number, playing: boolean) => {
    if (!timeline) return;
    for (const tr of timeline.tracks) {
      for (const c of tr.clips) {
        const el = mediaRefs.current.get(c.id);
        if (!el) continue;
        const offset = (c.offset ?? 0) + (t - c.start);
        const inside = t >= c.start && t < c.end && (!Number.isFinite(el.duration) || offset < el.duration);
        if (tr.kind === "audio") el.volume = Math.max(0, Math.min(1, tr.gain ?? 1));
        else el.hidden = !inside;
        if (inside && Math.abs(el.currentTime - offset) > SYNC_TOLERANCE_S) el.currentTime = offset;
        if (inside && playing) {
          if (el.paused) el.play().catch(() => {});
        } else if (!el.paused) {
          el.pause();
        }
      }
    }
  };

  const latest = useRef({ sync: syncMedia, duration: 0 });
  useEffect(() => {
    latest.current = { sync: syncMedia, duration: timeline?.duration ?? 0 };
  });
  useEffect(() => {
    if (!playing) return;
    let raf = 0;
    let last = performance.now();
    const tick = (now: number) => {
      const t = timeRef.current + (now - last) / 1000;
      last = now;
      const { sync, duration } = latest.current;
      if (duration > 0 && t >= duration) {
        setTimeAt(duration);
        setPlaying(false);
        return;
      }
      setTimeAt(t);
      sync(t, true);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(raf);
      latest.current.sync(timeRef.current, false);
    };
  }, [playing]);
  useEffect(() => {
    syncMedia(timeRef.current, false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [timeline]);
  useEffect(() => {
    syncMedia(timeRef.current, playing);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hiddenLanes]);

  const load = useCallback(
    async (pick?: string) => {
      if (!project) return;
      try {
        const list = await api.listTimelines(project);
        setDir(list.dir);
        setNames(list.names);
        api
          .listProjectMedia(project)
          .then((files) => setMedia(files.filter((f) => MEDIA_EXT.test(f.name))))
          .catch(() => setMedia([]));
        const chosen =
          pick && list.names.includes(pick) ? pick : list.names.includes(name) ? name : (list.names[0] ?? "");
        setName(chosen);
        if (!chosen) {
          setTimeline(null);
          return;
        }
        setTimeline(parseTimeline(await api.readTimeline(project, chosen)));
        setLoadError("");
      } catch (e) {
        setLoadError(String(e));
      }
    },
    [project, name],
  );

  useEffect(() => {
    load();
    api
      .listVoiceSamples()
      .then(setSamples)
      .catch(() => setSamples([]));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project]);

  const running = runningIds.size;

  // ponytail: a half-written JSON can briefly fail to parse; the trailing
  // debounce makes it rare. Retry once on parse error if it shows up.
  useEffect(() => {
    if (!project) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    api.watchProject(project).catch(() => {});
    const unlisten = listen<string>("project-changed", (e) => {
      if (e.payload !== project) return;
      clearTimeout(timer);
      timer = setTimeout(() => {
        if (!dirtyRef.current) load();
      }, RELOAD_DEBOUNCE_MS);
    });
    return () => {
      clearTimeout(timer);
      unlisten.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project]);

  useEffect(() => {
    if (!dirtyRef.current || !timeline || !project || !name) return;
    const t = setTimeout(() => {
      api
        .writeTimeline(project, name, serializeTimeline(timeline))
        .then(() => {
          dirtyRef.current = false;
        })
        .catch((e) => setError(String(e)));
    }, SAVE_DEBOUNCE_MS);
    return () => clearTimeout(t);
  }, [timeline, project, name, setError]);

  const update = (next: Timeline) => {
    dirtyRef.current = true;
    if (!name) setName(DEFAULT_TIMELINE);
    setTimeline(next);
  };

  const toggleLane = (id: string) =>
    setHiddenLanes((h) => {
      const next = new Set(h);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const seek = (t: number) => {
    setTimeAt(t);
    syncMedia(t, playing);
  };

  const exportVideo = () => {
    if (!name) return;
    setExporting(true);
    setStatus("Exporting video...");
    api
      .exportTimeline(project!, name)
      .then((out) => {
        setStatus(`Exported ${out}`);
        return api.revealProjectFile(project!, out);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setExporting(false));
  };

  const togglePlay = useCallback(() => setPlaying((p) => !p), []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.code !== "Space" || isEditable(e.target) || e.defaultPrevented) return;
      e.preventDefault();
      togglePlay();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [togglePlay]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.key !== "Backspace" && e.key !== "Delete") || isEditable(e.target) || !timeline || !selected) return;
      e.preventDefault();
      update(removeClip(timeline, selected.track, selected.clip));
      setSelected(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [timeline, selected]);

  const splitAtPlayhead = () => {
    if (!timeline) return;
    let next = timeline;
    for (const tr of timeline.tracks) {
      for (const c of tr.clips) next = splitClip(next, tr.id, c.id, time) ?? next;
    }
    if (next === timeline) return;
    update(next);
    const halved = next.tracks.find((tr) => tr.id === selected?.track)?.clips.find((c) => c.start === time);
    if (halved) setSelected({ track: selected!.track, clip: halved.id });
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const blade = (e.key === "b" || e.key === "B") && (e.ctrlKey || e.metaKey);
      if ((!blade && e.key !== "s" && e.key !== "S") || isEditable(e.target) || e.defaultPrevented) return;
      e.preventDefault();
      splitAtPlayhead();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  });

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setLaneWidth(el.clientWidth));
    ro.observe(el);
    setLaneWidth(el.clientWidth);
    return () => ro.disconnect();
  }, [project]);

  if (!project) return null;

  const shown = timeline ?? emptyTimeline();
  const source = timeline ? videoSource(timeline) : undefined;
  const captionTr = timeline ? captionTrack(timeline) : undefined;
  const captions = captionTr && !hiddenLanes.has(captionTr.id) ? captionTr : undefined;
  const activeCaption = captions?.clips.find((c) => time >= c.start && time < c.end);
  const clipsOf = (kind: TrackKind, resolve: (src: string) => string | null) =>
    (timeline?.tracks ?? [])
      .filter((tr) => tr.kind === kind && !hiddenLanes.has(tr.id))
      .flatMap((tr) => tr.clips)
      .flatMap((c) => (c.src ? [{ clip: c, src: resolve(c.src) }] : []))
      .filter((c): c is { clip: Clip; src: string } => !!c.src);
  const clipAudio = clipsOf("audio", (src) => clipSrc(dir, src));
  const clipVideo = clipsOf("video", (src) => safeProjectMediaSrc(resolveSrc(dir, src)));
  const clipOverlays = clipsOf("overlay", (src) => safeProjectMediaSrc(resolveSrc(dir, src)));
  const playable = clipVideo.length > 0 || clipAudio.length > 0;
  // A hidden video lane keeps the stage up (over black) so overlays and captions can be watched alone.
  const hasHiddenVideo = (timeline?.tracks ?? []).some(
    (tr) => tr.kind === "video" && hiddenLanes.has(tr.id) && tr.clips.some((c) => c.src),
  );
  const duration = shown.duration;
  const track = timeline && selected ? timeline.tracks.find((t) => t.id === selected.track) : undefined;
  const clip = track?.clips.find((c) => c.id === selected?.clip);
  const drafts = timeline ? draftCount(timeline) : 0;
  const hasVoice = timeline ? spokenCount(timeline) : 0;
  const overlays = timeline ? overlayCount(timeline) : 0;
  const videoCount = timeline
    ? timeline.tracks.filter((tr) => tr.kind === "video").flatMap((tr) => tr.clips.filter((c) => c.src)).length
    : 0;

  const fitPps = duration > 0 ? Math.max(MIN_PPS, (laneWidth - 24) / duration) : 40;
  const pps = zoom ?? fitPps;
  const contentWidth = Math.max(laneWidth, duration * pps + 24);
  const step = rulerStep(pps);
  const ticks = Array.from({ length: Math.floor(duration / step) + 1 }, (_, i) => i * step);
  const timeAt = (clientX: number) => {
    const el = scrollRef.current;
    if (!el) return 0;
    const t = (el.scrollLeft + clientX - el.getBoundingClientRect().left) / pps;
    return Math.max(0, Math.min(duration, t));
  };
  const zoomBy = (factor: number, aroundX?: number) => {
    const el = scrollRef.current;
    const next = Math.min(MAX_PPS, Math.max(MIN_PPS, pps * factor));
    if (el && aroundX !== undefined) {
      const t = (el.scrollLeft + aroundX) / pps;
      requestAnimationFrame(() => {
        el.scrollLeft = t * next - aroundX;
      });
    }
    setZoom(next);
  };
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      zoomBy(e.deltaY < 0 ? 1.15 : 1 / 1.15, e.clientX - el.getBoundingClientRect().left);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  });

  const beginDrag = (e: ReactPointerEvent<HTMLElement>, kind: "move" | "start" | "end", tr: Track, c: Clip) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    setSelected({ track: tr.id, clip: c.id });
    dragClipRef.current = { kind, track: tr.id, clip: c, x0: e.clientX };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const dragTo = (e: ReactPointerEvent<HTMLElement>, c: Clip) => {
    const d = dragClipRef.current;
    if (!d || d.clip.id !== c.id) return;
    const dx = (e.clientX - d.x0) / pps;
    const tol = SNAP_PX / pps;
    const points = snapPoints(shown, c.id, time);
    if (d.kind === "move") {
      const len = d.clip.end - d.clip.start;
      const start = snapTime(d.clip.start + dx, points, tol);
      const end = snapTime(d.clip.end + dx, points, tol);
      update(moveClip(shown, d.track, c.id, start !== d.clip.start + dx ? start : end - len));
      return;
    }
    const base = d.kind === "start" ? d.clip.start : d.clip.end;
    const el = mediaRefs.current.get(c.id);
    const sourceLength = el && Number.isFinite(el.duration) && el.duration > 0 ? el.duration : undefined;
    update(trimClip(shown, d.track, c.id, d.kind, snapTime(base + dx, points, tol), sourceLength));
  };
  const endDrag = () => {
    dragClipRef.current = null;
  };

  const generate = () => {
    const mode = timeline?.source_audio ?? "mute";
    const prompt = hasVoice
      ? `Redo the draft clips in ${name}. Use the voice sample each clip names, and ask me only about clips that name none. ${sourceAudioInstruction(mode)}`
      : `Add my cloned voice to ${source ?? "the video in this project"}: write ${name || "<stem>.timeline.json"} and make the audio for every clip. ${sourceAudioInstruction(mode)}`;
    promptProject(project, prompt).catch((e) => setError(String(e)));
  };

  const addLaneClip = (tr: Track) => {
    const before = new Set(tr.clips.map((c) => c.id));
    const next = addEmptyClip(shown, tr.id, time);
    update(next);
    const fresh = next.tracks.find((t) => t.id === tr.id)?.clips.find((c) => !before.has(c.id));
    if (fresh) setSelected({ track: tr.id, clip: fresh.id });
  };

  const captionize = () => {
    const style = captionStyle(captionTrack(shown)?.style);
    const prompt = `Add captions to ${name || DEFAULT_TIMELINE} in the "${style}" style: write a captions track following the captions rules in the skill, and leave every other track alone.`;
    promptProject(project, prompt).catch((e) => setError(String(e)));
  };

  const redoClip = (trackId: string, c: Clip) => {
    if (!timeline || !name) return;
    const track = timeline.tracks.find((tr) => tr.id === trackId);
    const sample = track && clipSample(track, c);
    const voice = sample ? `and the voice sample ${sample}` : "and ask me which voice sample to use first";
    const next = setClipText(timeline, trackId, c.id, c.text ?? "");
    dirtyRef.current = false;
    setTimeline(next);
    api
      .writeTimeline(project, name, serializeTimeline(next))
      .then(() =>
        promptProject(
          project,
          `Redo only the voice of clip ${c.id} in ${name}, using its current text ${voice}. Leave every other clip untouched.`,
        ),
      )
      .catch((e) => setError(String(e)));
  };

  const speakCaption = (trackId: string, c: Clip) => {
    if (!timeline) return;
    update(speakClip(timeline, trackId, c.id));
    setStatus('Draft voice clip added: use "Redo drafts" to voice it');
  };

  const importFiles = async (files: FileList) => {
    const media = Array.from(files).filter((f) => MEDIA_EXT.test(f.name));
    if (media.length === 0) return;
    setImporting(true);
    try {
      for (const f of media) {
        setStatus(`Importing ${f.name}...`);
        await api.importProjectFile(project, f.name, await f.arrayBuffer());
      }
      setStatus(`Imported ${media.length} file${media.length === 1 ? "" : "s"}`);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setImporting(false);
    }
  };

  const addRecording = () => {
    api
      .addProjectVideo(project)
      .then((added) => {
        if (added) load();
      })
      .catch((e) => setError(String(e)));
  };

  const addVoiceTo = (video: string) => {
    const target =
      timeline && source === video && name
        ? name
        : `${video.replace(/^media\//, "").replace(VIDEO_EXT, "")}.timeline.json`;
    const prompt = `Add my cloned voice to ${video}: write ${target} and make the audio for every clip. Set "source_audio" yourself: "transcribe" only if the recording has speech that whisper can transcribe, otherwise "mute" and narrate what happens on screen from the keyframes. Never boost, filter or retry the audio.`;
    promptProject(project, prompt).catch((e) => setError(String(e)));
  };

  const laneAccepts = (tr: Track, file: string | null) =>
    !!file && tr.kind !== "captions" && (VIDEO_EXT.test(file) ? tr.kind !== "audio" : tr.kind === "audio");

  const dropOn = (tr: Track, e: React.DragEvent<HTMLDivElement>) => {
    const file = dragRef.current;
    dragRef.current = null;
    setDropLane(null);
    if (!laneAccepts(tr, file)) return;
    e.preventDefault();
    const at = duration > 0 ? timeAt(e.clientX) : 0;
    update(addClip(shown, tr.id, file!, durations[file!] ?? FALLBACK_CLIP_S, at));
  };

  return (
    <div id="timeline-view" className="flex min-w-0 flex-1 flex-col overflow-y-auto bg-background">
      <div className="flex flex-wrap items-center gap-2 border-b border-border px-4 py-2">
        {names.length > 1 ? (
          <select
            aria-label="Timeline"
            value={name}
            onChange={(e) => load(e.target.value)}
            className="h-8 rounded-md border border-input bg-transparent px-1 text-[0.85rem] font-semibold text-foreground"
          >
            {names.map((n) => (
              <option key={n} value={n}>
                {n.replace(/\.timeline\.json$/, "")}
              </option>
            ))}
          </select>
        ) : (
          <h2 className="truncate text-[0.95rem] font-semibold">
            {name ? name.replace(/\.timeline\.json$/, "") : project}
          </h2>
        )}
        {timeline && (
          <>
            <span className="font-mono text-[0.75rem] tabular-nums text-muted-foreground">
              {fmtTime(time)} / {fmtTime(duration)}
            </span>
            <button
              aria-label={playing ? "Pause" : "Play"}
              title="Play / pause (Space)"
              disabled={!playable}
              onClick={togglePlay}
              className="inline-flex size-7 items-center justify-center rounded-md border border-input text-foreground hover:bg-primary/10 disabled:opacity-40"
            >
              {playing ? <Pause size={13} /> : <Play size={13} />}
            </button>
            {drafts > 0 && (
              <span className="rounded-full bg-amber-500/20 px-2 py-0.5 text-[0.7rem] font-medium text-amber-600 dark:text-amber-400">
                {drafts} draft{drafts === 1 ? "" : "s"}
              </span>
            )}
          </>
        )}
        <div className="ml-auto flex flex-wrap items-center gap-1.5">
          {timeline && (
            <>
              <div
                role="group"
                aria-label="Frame size"
                title="The exported frame size: the recording is scaled to fit and padded, cards are placed in this frame"
                className="flex items-center gap-0.5 rounded-lg border border-input p-0.5"
              >
                {RESOLUTIONS.map((o) => {
                  const ratio = frameAspect({ resolution: o.value });
                  const Icon = ratio > 1 ? RectangleHorizontal : ratio < 1 ? RectangleVertical : Square;
                  const on = frameSize(timeline) === o.value;
                  return (
                    <Button
                      key={o.value}
                      size="icon-sm"
                      variant={on ? "secondary" : "ghost"}
                      aria-pressed={on}
                      aria-label={o.label}
                      title={`Frame ${o.label}`}
                      className={on ? undefined : "text-muted-foreground"}
                      onClick={() => update({ ...timeline, resolution: o.value })}
                    >
                      <Icon size={14} />
                    </Button>
                  );
                })}
              </div>
              <div
                role="group"
                aria-label="Recording audio"
                title="What happens to the recording's own soundtrack: the agent transcribes it and re-voices it with your clone, it is dropped, or it plays under the voice clips"
                className="flex items-center gap-0.5 rounded-lg border border-input p-0.5"
              >
                {SOURCE_AUDIO.map((o) => {
                  const Icon = AUDIO_ICON[o.value];
                  const on = (timeline.source_audio ?? "mute") === o.value;
                  return (
                    <Button
                      key={o.value}
                      size="icon-sm"
                      variant={on ? "secondary" : "ghost"}
                      aria-pressed={on}
                      aria-label={o.label}
                      title={`Recording audio: ${o.label}`}
                      className={on ? undefined : "text-muted-foreground"}
                      onClick={() => update({ ...timeline, source_audio: o.value })}
                    >
                      <Icon size={14} />
                    </Button>
                  );
                })}
              </div>
              <span className="mx-0.5 h-5 w-px bg-border" />
              <Button
                variant="outline"
                size="icon-sm"
                aria-label="Split"
                title="Split the clips under the playhead in two (S or Ctrl+B)"
                onClick={splitAtPlayhead}
              >
                <Scissors size={14} />
              </Button>
              <Button
                variant="outline"
                size="icon-sm"
                aria-label="Add captions"
                title="Ask for captions from the voice clips or the recording's transcript"
                onClick={captionize}
                disabled={running > 0}
              >
                <Type size={14} />
              </Button>
              <Button
                size="icon-sm"
                aria-label={hasVoice ? "Redo drafts" : "Add voice"}
                title={hasVoice ? "Redo drafts" : "Add voice"}
                onClick={generate}
                disabled={running > 0}
              >
                <Sparkles size={14} />
              </Button>
              <Button
                variant="outline"
                size="icon-sm"
                aria-label="Export"
                title={exporting ? "Exporting..." : "Render the timeline to an MP4 with ffmpeg"}
                onClick={exportVideo}
                disabled={exporting || running > 0 || (hasVoice === 0 && overlays === 0 && videoCount === 0)}
              >
                {exporting ? <Loader2 size={14} className="animate-spin" /> : <Download size={14} />}
              </Button>
            </>
          )}
        </div>
      </div>

      <div className="flex flex-col gap-3 p-4">
        {loadError && <p className="text-[0.8rem] text-destructive">{loadError}</p>}

        <div className="flex max-h-[50vh] min-h-[200px] w-full items-center justify-center overflow-hidden rounded-lg bg-black">
          {timeline && (clipVideo.length > 0 || hasHiddenVideo) ? (
            <div
              ref={stageRef}
              className="relative"
              style={{
                aspectRatio: frameAspect(timeline),
                width: `min(100%, calc(50vh * ${frameAspect(timeline)}))`,
                containerType: "size",
              }}
            >
              {clipVideo.map(({ clip: c, src }) => (
                <video
                  key={c.id}
                  src={src}
                  muted={timeline.source_audio !== "keep"}
                  playsInline
                  preload="auto"
                  hidden
                  ref={(el) => {
                    if (el) mediaRefs.current.set(c.id, el);
                    else mediaRefs.current.delete(c.id);
                  }}
                  onLoadedMetadata={() => syncMedia(timeRef.current, playing)}
                  className="pointer-events-none absolute inset-0 size-full object-contain"
                />
              ))}
              {clipOverlays.map(({ clip: c, src }) => (
                <video
                  key={c.id}
                  src={src}
                  muted
                  playsInline
                  preload="auto"
                  hidden
                  ref={(el) => {
                    if (el) mediaRefs.current.set(c.id, el);
                    else mediaRefs.current.delete(c.id);
                  }}
                  className="pointer-events-none absolute"
                  style={{
                    left: `${(c.x ?? 0) * 100}%`,
                    top: `${(c.y ?? 0) * 100}%`,
                    width: c.width === undefined ? (c.height === undefined ? "100%" : "auto") : `${c.width * 100}%`,
                    height: c.height === undefined ? "auto" : `${c.height * 100}%`,
                  }}
                />
              ))}
              {captions && activeCaption && (
                <CaptionOverlay
                  track={captions}
                  clip={activeCaption}
                  now={time}
                  stage={stageRef}
                  onMove={(x, y) => update(moveCaptions(shown, captions.id, x, y))}
                />
              )}
            </div>
          ) : timeline && source ? (
            <p className="p-6 text-center text-[0.8rem] text-muted-foreground">
              Preview unavailable for {source} (only files under the default projects root can be previewed).
            </p>
          ) : (
            <p className="max-w-md p-6 text-center text-[0.8rem] text-muted-foreground">
              {media.length === 0
                ? "No media yet. Drop files on the media pool to start."
                : 'Nothing on the timeline yet. Ask the agent to arrange the media pool, e.g. "put intro.mp4 first, then demo.mov, with music.mp3 underneath".'}
            </p>
          )}
        </div>
        {clipAudio.map(({ clip: c, src }) => (
          <audio
            key={c.id}
            src={src}
            preload="auto"
            ref={(el) => {
              if (el) mediaRefs.current.set(c.id, el);
              else mediaRefs.current.delete(c.id);
            }}
          />
        ))}

        <div className="flex overflow-hidden rounded-md border border-zinc-800 bg-[#141416] text-zinc-200">
          <div className="flex w-40 shrink-0 flex-col border-r border-zinc-800 bg-[#1b1b1e]">
            <div className="flex h-7 items-center justify-end gap-0.5 border-b border-zinc-800 px-1">
              <button
                aria-label="Zoom out"
                title="Zoom out (⌘/Ctrl + wheel)"
                onClick={() => zoomBy(1 / ZOOM_STEP)}
                className={ZOOM_BTN}
              >
                <ZoomOut size={12} />
              </button>
              <button
                aria-label="Zoom in"
                title="Zoom in (⌘/Ctrl + wheel)"
                onClick={() => zoomBy(ZOOM_STEP)}
                className={ZOOM_BTN}
              >
                <ZoomIn size={12} />
              </button>
              <button
                aria-label="Zoom to fit"
                title="Zoom to fit"
                onClick={() => setZoom(null)}
                className={cn(ZOOM_BTN, "px-1 text-[0.62rem] font-medium")}
              >
                Fit
              </button>
            </div>
            {laneOrder(shown.tracks).map((tr) => (
              <div
                key={tr.id}
                className="flex h-14 items-center gap-1.5 border-b border-zinc-800/70 px-2 text-[0.72rem] font-medium"
              >
                <span className={cn("h-8 w-1 shrink-0 rounded-sm", TRACK_SWATCH[tr.kind])} />
                <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="truncate">
                    {TRACK_LABEL[tr.kind]} {tr.id.startsWith(tr.kind) ? tr.id.slice(tr.kind.length) : ""}
                  </span>
                  {tr.kind === "audio" && (tr.voice_sample || tr.clips.some(isSpoken)) && (
                    <VoiceSelect
                      label={`Voice sample for ${tr.id}`}
                      title="The voice sample this track's speech is cloned from (recorded in Settings > Voice samples)"
                      samples={samples}
                      value={tr.voice_sample}
                      onChange={(sample) =>
                        update({
                          ...shown,
                          tracks: shown.tracks.map((t) => (t.id === tr.id ? { ...t, voice_sample: sample } : t)),
                        })
                      }
                      className="h-5 w-full truncate rounded border border-zinc-700 bg-zinc-900 px-1 text-[0.65rem] font-normal text-zinc-300"
                    />
                  )}
                  {tr.kind === "captions" && (
                    <select
                      aria-label={`Caption style for ${tr.id}`}
                      title="The caption look: the preview and the burned-in export share these presets. Highlight and Karaoke need per-word timing from the agent"
                      value={captionStyle(tr.style)}
                      onChange={(e) =>
                        update({
                          ...shown,
                          tracks: shown.tracks.map((t) => (t.id === tr.id ? { ...t, style: e.target.value } : t)),
                        })
                      }
                      className="h-5 w-full truncate rounded border border-zinc-700 bg-zinc-900 px-1 text-[0.65rem] font-normal text-zinc-300"
                    >
                      {CAPTION_STYLES.map((s) => (
                        <option key={s.value} value={s.value}>
                          Captions: {s.label}
                        </option>
                      ))}
                    </select>
                  )}
                </div>
                {(tr.kind === "audio" || tr.kind === "captions") && (
                  <button
                    aria-label={tr.kind === "captions" ? "Add caption" : "Add voice clip"}
                    title={
                      tr.kind === "captions"
                        ? "Add a caption at the playhead"
                        : "Add a clip at the playhead for the agent to voice"
                    }
                    onClick={() => addLaneClip(tr)}
                    className="shrink-0 rounded p-0.5 text-zinc-400 hover:bg-white/10 hover:text-zinc-100"
                  >
                    <Plus size={14} />
                  </button>
                )}
                <button
                  aria-label={`${hiddenLanes.has(tr.id) ? "Show" : "Hide"} ${tr.kind} lane ${tr.id}`}
                  title={hiddenLanes.has(tr.id) ? "Show this lane in the preview" : "Hide this lane from the preview"}
                  onClick={() => toggleLane(tr.id)}
                  className={cn(
                    "shrink-0 rounded p-0.5 hover:bg-white/10",
                    hiddenLanes.has(tr.id) ? "text-zinc-600 hover:text-zinc-300" : "text-zinc-400 hover:text-zinc-100",
                  )}
                >
                  {hiddenLanes.has(tr.id) ? <EyeOff size={14} /> : <Eye size={14} />}
                </button>
                <TrackIcon kind={tr.kind} />
              </div>
            ))}
            <div className="flex h-8 items-center gap-1 px-1.5">
              {(["video", "audio", "overlay"] as TrackKind[])
                .concat(shown.tracks.some((tr) => tr.kind === "captions") ? [] : (["captions"] as TrackKind[]))
                .map((k) => (
                  <button
                    key={k}
                    aria-label={`Add ${k} track`}
                    title={`Add ${k} track`}
                    onClick={() => update(addTrack(shown, k))}
                    className="inline-flex items-center gap-0.5 rounded px-1 py-0.5 text-[0.68rem] text-zinc-400 hover:bg-white/10 hover:text-zinc-100"
                  >
                    <Plus size={10} /> <TrackIcon kind={k} />
                  </button>
                ))}
            </div>
          </div>
          <div ref={scrollRef} className="relative min-w-0 flex-1 overflow-x-auto overflow-y-hidden">
            <div className="relative" style={{ width: contentWidth }}>
              <div
                role="slider"
                aria-label="Playhead"
                aria-valuemin={0}
                aria-valuemax={duration}
                aria-valuenow={time}
                className="relative h-7 cursor-ew-resize touch-none border-b border-zinc-800 bg-[#1b1b1e] font-mono text-[0.6rem] text-zinc-400 select-none"
                onPointerDown={(e) => {
                  if (e.button !== 0) return;
                  e.preventDefault();
                  scrubRef.current = true;
                  e.currentTarget.setPointerCapture(e.pointerId);
                  seek(timeAt(e.clientX));
                }}
                onPointerMove={(e) => scrubRef.current && seek(timeAt(e.clientX))}
                onPointerUp={() => {
                  scrubRef.current = false;
                }}
                onPointerCancel={() => {
                  scrubRef.current = false;
                }}
              >
                {ticks.map((t) => (
                  <span
                    key={t}
                    className="absolute bottom-0 h-3 border-l border-zinc-600 pl-1 leading-3"
                    style={{ left: t * pps }}
                  >
                    {fmtTime(t)}
                  </span>
                ))}
                {ticks.flatMap((t) =>
                  [1, 2, 3].map((k) => (
                    <span
                      key={`${t}-${k}`}
                      className="absolute bottom-0 h-1.5 border-l border-zinc-700"
                      style={{ left: (t + (step * k) / 4) * pps }}
                    />
                  )),
                )}
              </div>
              {laneOrder(shown.tracks).map((tr) => (
                <div
                  key={tr.id}
                  aria-label={`${TRACK_LABEL[tr.kind]} lane ${tr.id}`}
                  onDragOver={(e) => {
                    if (!laneAccepts(tr, dragRef.current)) return;
                    e.preventDefault();
                    e.dataTransfer.dropEffect = "copy";
                    if (dropLane !== tr.id) setDropLane(tr.id);
                  }}
                  onDragLeave={() => dropLane === tr.id && setDropLane(null)}
                  onDrop={(e) => dropOn(tr, e)}
                  className={cn(
                    "relative h-14 border-b border-zinc-800/70",
                    dropLane === tr.id && "bg-primary/15 ring-1 ring-inset ring-primary",
                  )}
                >
                  {tr.clips.map((c) => {
                    const isSel = selected?.track === tr.id && selected.clip === c.id;
                    const layout = clipLayout(c, pps);
                    const mediaSrc = c.src
                      ? tr.kind === "audio"
                        ? clipSrc(dir, c.src)
                        : safeProjectMediaSrc(resolveSrc(dir, c.src))
                      : null;
                    const sample = clipSample(tr, c);
                    return (
                      <div key={c.id} className="group absolute top-1 bottom-1" style={layout}>
                        <button
                          title={[c.text || c.src || c.id, sample && `Voice: ${sample}`].filter(Boolean).join("\n")}
                          aria-pressed={isSel}
                          onPointerDown={(e) => beginDrag(e, "move", tr, c)}
                          onPointerMove={(e) => dragTo(e, c)}
                          onPointerUp={endDrag}
                          onPointerCancel={endDrag}
                          className={cn(
                            "absolute inset-0 touch-none overflow-hidden rounded-[3px] border text-left text-[0.68rem] text-white/95 outline-none select-none cursor-grab active:cursor-grabbing",
                            clipClass(tr, c),
                            isSel && "ring-2 ring-white",
                          )}
                        >
                          {mediaSrc &&
                            (tr.kind !== "audio" ? (
                              <Thumbnails
                                src={mediaSrc}
                                start={c.offset ?? 0}
                                length={c.end - c.start}
                                width={layout.width}
                                height={CLIP_H}
                              />
                            ) : (
                              <Waveform
                                src={mediaSrc}
                                offset={c.offset ?? 0}
                                length={c.end - c.start}
                                width={layout.width}
                                height={CLIP_H}
                              />
                            ))}
                          <span className="relative block truncate bg-black/35 px-1.5 leading-5">
                            {c.text || c.src?.replace(/^media\//, "") || c.id}
                          </span>
                          {sample && (
                            <span
                              aria-hidden="true"
                              className="absolute inset-x-0 bottom-0 h-1"
                              style={{ background: sampleColour(sample) }}
                            />
                          )}
                          <span
                            aria-hidden="true"
                            className="absolute inset-y-0 left-0 w-1.5 cursor-ew-resize hover:bg-white/40"
                            onPointerDown={(e) => beginDrag(e, "start", tr, c)}
                          />
                          <span
                            aria-hidden="true"
                            className="absolute inset-y-0 right-0 w-1.5 cursor-ew-resize hover:bg-white/40"
                            onPointerDown={(e) => beginDrag(e, "end", tr, c)}
                          />
                        </button>
                        {tr.kind === "video" && c.src && (
                          <button
                            aria-label={`Add voice to ${c.src}`}
                            title={
                              hasVoice ? "Redo the voice for this recording" : "Add my cloned voice to this recording"
                            }
                            disabled={running > 0}
                            onClick={() => addVoiceTo(c.src!)}
                            className="absolute top-1 right-1 z-10 hidden size-6 items-center justify-center rounded-md bg-primary text-primary-foreground shadow group-hover:flex hover:bg-primary-hover disabled:opacity-50"
                          >
                            <Sparkles size={13} />
                          </button>
                        )}
                      </div>
                    );
                  })}
                </div>
              ))}
              {duration > 0 && (
                <div className="pointer-events-none absolute top-0 bottom-0 z-10" style={{ left: time * pps }}>
                  <div className="absolute top-0 -ml-[5px] h-0 w-0 border-x-[5px] border-t-[7px] border-x-transparent border-t-red-500" />
                  <div className="absolute top-0 bottom-0 w-px bg-red-500" />
                </div>
              )}
            </div>
          </div>
        </div>

        <div
          aria-label="Media pool"
          onDragOver={(e) => {
            if (dragRef.current || !e.dataTransfer.types.includes("Files")) return;
            e.preventDefault();
            e.dataTransfer.dropEffect = "copy";
            if (!poolOver) setPoolOver(true);
          }}
          onDragLeave={() => setPoolOver(false)}
          onDrop={(e) => {
            if (dragRef.current) return;
            e.preventDefault();
            setPoolOver(false);
            importFiles(e.dataTransfer.files);
          }}
          className={cn(
            "flex flex-col gap-1 rounded-lg border border-border bg-secondary/30 p-3",
            poolOver && "border-dashed border-ring bg-primary/10",
          )}
        >
          <div className="flex items-center gap-2">
            <h3 className="text-[0.8rem] font-semibold" title="The project's media folder">
              Media pool
            </h3>
            <span className="text-[0.72rem] text-muted-foreground">
              {media.length} file{media.length === 1 ? "" : "s"}
            </span>
            <Button variant="outline" size="sm" className="ml-auto" onClick={addRecording}>
              <FilePlus size={14} /> Add recording
            </Button>
          </div>
          {importing && <p className="text-[0.78rem] text-muted-foreground">Importing...</p>}
          {media.length === 0 && !importing && (
            <p className="text-[0.78rem] text-muted-foreground">
              No media yet. Drop video or audio files here, or add a recording. Files live in the project's media
              folder.
            </p>
          )}
          {media.map((f) => {
            const video = VIDEO_EXT.test(f.name);
            const src = safeProjectMediaSrc(resolveSrc(dir, f.name));
            return (
              <div key={f.name} className="flex flex-col gap-1">
                <div className="flex items-center gap-2 text-[0.8rem]">
                  <button
                    title="Drag onto a lane, or click to select"
                    draggable
                    onDragStart={(e) => {
                      dragRef.current = f.name;
                      e.dataTransfer.setData("text/plain", f.name);
                      e.dataTransfer.effectAllowed = "copy";
                    }}
                    onDragEnd={() => {
                      dragRef.current = null;
                      setDropLane(null);
                    }}
                    onClick={() => setSelectedFile(f.name)}
                    className={cn(
                      "flex min-w-0 flex-1 items-center gap-2 rounded px-1.5 py-1 text-left hover:bg-primary/10",
                      selectedFile === f.name && "bg-primary/15",
                    )}
                  >
                    {video ? (
                      <Film size={13} className="shrink-0 text-primary" />
                    ) : (
                      <Music size={13} className="shrink-0 text-emerald-500" />
                    )}
                    <span className="truncate">{f.name}</span>
                    <span className="ml-auto shrink-0 font-mono text-[0.7rem] tabular-nums text-muted-foreground">
                      {durations[f.name] !== undefined && `${fmtTime(durations[f.name])} · `}
                      {fmtBytes(f.size)}
                    </span>
                  </button>
                  {src && (
                    <video
                      src={src}
                      preload="metadata"
                      className="hidden"
                      onLoadedMetadata={(e) => {
                        const d = e.currentTarget.duration;
                        setDurations((prev) => ({ ...prev, [f.name]: d }));
                      }}
                    />
                  )}
                </div>
                {!video && src && selectedFile === f.name && (
                  <AudioPlayer src={src} ariaLabel={f.name} path={resolveSrc(dir, f.name)} />
                )}
              </div>
            );
          })}
        </div>

        {timeline && track && clip && (
          <ClipEditor
            track={track}
            clip={clip}
            dir={dir}
            timeline={timeline}
            samples={samples}
            onChange={update}
            onRedo={running > 0 ? undefined : () => redoClip(track.id, clip)}
            onSpeak={running > 0 ? undefined : () => speakCaption(track.id, clip)}
          />
        )}
      </div>
    </div>
  );
}

function ClipEditor({
  track,
  clip,
  dir,
  timeline,
  samples,
  onChange,
  onRedo,
  onSpeak,
}: {
  track: Track;
  clip: Clip;
  dir: string;
  timeline: Timeline;
  samples: VoiceSample[];
  onChange: (t: Timeline) => void;
  onRedo?: () => void;
  onSpeak?: () => void;
}) {
  const audio = clip.src && track.kind !== "video" ? safeAudioSrc(resolveSrc(dir, clip.src)) : null;
  const spoken = track.kind === "audio" && isSpoken(clip);
  const caption = track.kind === "captions";
  const sample = clipSample(track, clip);
  return (
    <div className="flex flex-col gap-2 rounded-lg border border-border bg-secondary/40 p-3">
      <div className="flex items-center gap-2 text-[0.75rem] text-muted-foreground">
        <span className="font-medium text-foreground">Clip {clip.id}</span>
        <span>
          {fmtTime(clip.start)} - {fmtTime(clip.end)}
        </span>
        {clip.status && <span className="rounded border border-border px-1">{clip.status}</span>}
        {spoken && (
          <span className="flex items-center gap-1">
            {sample && <span className="size-2 rounded-full" style={{ background: sampleColour(sample) }} />}
            <VoiceSelect
              label={`Voice sample for ${clip.id}`}
              title="The voice sample this clip is cloned from: pick another and redo the voice"
              samples={samples}
              value={sample}
              onChange={(pick) => onChange(setClipSample(timeline, track.id, clip.id, pick))}
              className="h-6 rounded border border-input bg-transparent px-1 text-[0.72rem] text-foreground"
            />
          </span>
        )}
        {spoken && (
          <Button
            variant="outline"
            size="sm"
            className="ml-auto h-6 px-2 text-[0.72rem]"
            title="Generate this clip's voice again with the text below"
            disabled={!onRedo}
            onClick={onRedo}
          >
            <RefreshCw size={12} /> Redo voice
          </Button>
        )}
        {caption && (
          <Button
            variant="outline"
            size="sm"
            className="ml-auto h-6 px-2 text-[0.72rem]"
            title='Voice this caption: a draft audio clip with this text and range, "Redo drafts" synthesizes it'
            disabled={!onSpeak}
            onClick={onSpeak}
          >
            <Sparkles size={12} /> Speak this
          </Button>
        )}
        <button
          aria-label={`Delete clip ${clip.id}`}
          title="Delete clip"
          onClick={() => onChange(removeClip(timeline, track.id, clip.id))}
          className={cn("text-muted-foreground hover:text-destructive", !spoken && !caption && "ml-auto")}
        >
          <Trash2 size={14} />
        </button>
      </div>
      {(spoken || caption) && (
        <textarea
          id={`clip-text-${clip.id}`}
          aria-label={caption ? `Caption text for ${clip.id}` : `Voice text for ${clip.id}`}
          rows={3}
          value={clip.text ?? ""}
          placeholder={
            caption
              ? "What should this caption say?"
              : "What should be said here? Leave blank to let the agent suggest it."
          }
          onChange={(e) => onChange(setClipText(timeline, track.id, clip.id, e.target.value))}
          className="w-full resize-y rounded-lg border border-input bg-transparent px-2.5 py-1.5 text-[0.85rem] text-foreground outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
        />
      )}
      {audio && <AudioPlayer src={audio} ariaLabel={`Audio for ${clip.id}`} path={resolveSrc(dir, clip.src!)} />}
      {clip.src && !audio && track.kind !== "video" && (
        <p className="text-[0.75rem] text-muted-foreground">{clip.src}</p>
      )}
    </div>
  );
}
