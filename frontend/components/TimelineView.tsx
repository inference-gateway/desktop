import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  FilePlus,
  Film,
  FolderOpen,
  Layers,
  Music,
  Pause,
  Play,
  Plus,
  RefreshCw,
  Sparkles,
  Trash2,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { api, type ProjectFile, type VoiceSample } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { safeAudioSrc, safeProjectMediaSrc } from "@/lib/tools";
import {
  addClip,
  addMarker,
  addTrack,
  clipLayout,
  draftCount,
  emptyTimeline,
  fmtTime,
  isSpoken,
  laneOrder,
  moveClip,
  overlayCount,
  rulerStep,
  snapPoints,
  snapTime,
  trimClip,
  spokenCount,
  parseTimeline,
  removeClip,
  resolveSrc,
  serializeTimeline,
  setClipText,
  videoSource,
  SOURCE_AUDIO,
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
const TRACK_LABEL: Record<Track["kind"], string> = { video: "Video", audio: "Audio", overlay: "Overlay" };
const TRACK_SWATCH: Record<Track["kind"], string> = {
  video: "bg-sky-500",
  audio: "bg-emerald-500",
  overlay: "bg-fuchsia-500",
};
const TRACK_ICON: Record<Track["kind"], typeof Film> = { video: Film, audio: Music, overlay: Layers };
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

function TrackIcon({ kind }: { kind: TrackKind }) {
  const Icon = TRACK_ICON[kind];
  return <Icon size={11} className="shrink-0 text-zinc-500" />;
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
  const [poolOver, setPoolOver] = useState(false);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const videoRef = useRef<HTMLVideoElement>(null);
  const mediaRefs = useRef(new Map<string, HTMLMediaElement>());
  const { setStatus } = useDesktop();

  // The preview plays whatever the timeline holds: every audio clip and
  // overlay card is kept in step with the video's clock; the original sound
  // stays only with "keep". Overlays are shown only inside their range.
  const syncMedia = (t: number, playing: boolean) => {
    if (!timeline) return;
    for (const tr of timeline.tracks) {
      if (tr.kind === "video") continue;
      for (const c of tr.clips) {
        const el = mediaRefs.current.get(c.id);
        if (!el) continue;
        const offset = (c.offset ?? 0) + (t - c.start);
        const inside = t >= c.start && t < c.end && (!Number.isFinite(el.duration) || offset < el.duration);
        if (tr.kind === "overlay") el.hidden = !inside;
        else el.volume = Math.max(0, Math.min(1, tr.gain ?? 1));
        if (playing && inside) {
          if (Math.abs(el.currentTime - offset) > SYNC_TOLERANCE_S) el.currentTime = offset;
          if (el.paused) el.play().catch(() => {});
        } else if (!el.paused) {
          el.pause();
        }
      }
    }
  };

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

  // Reload whenever the project directory changes on disk (agent writes, new
  // media, external editors), unless local edits are pending.
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

  const seek = (t: number) => {
    const el = videoRef.current;
    if (el) el.currentTime = t;
    setTime(t);
    syncMedia(t, !!el && !el.paused);
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

  const togglePlay = useCallback(() => {
    const el = videoRef.current;
    if (!el) return;
    if (el.paused) el.play().catch(() => {});
    else el.pause();
  }, []);

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
      const tr = timeline.tracks.find((t) => t.id === selected.track);
      if (!tr || tr.kind === "video") return;
      e.preventDefault();
      update(removeClip(timeline, tr.id, selected.clip));
      setSelected(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [timeline, selected]);

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
  const videoPath = source;
  const videoSrc = videoPath ? safeProjectMediaSrc(resolveSrc(dir, videoPath)) : null;
  const clipsOf = (kind: TrackKind, resolve: (src: string) => string | null) =>
    (timeline?.tracks ?? [])
      .filter((tr) => tr.kind === kind)
      .flatMap((tr) => tr.clips)
      .flatMap((c) => (c.src ? [{ clip: c, src: resolve(c.src) }] : []))
      .filter((c): c is { clip: Clip; src: string } => !!c.src);
  const clipAudio = clipsOf("audio", (src) => clipSrc(dir, src));
  const clipOverlays = clipsOf("overlay", (src) => safeProjectMediaSrc(resolveSrc(dir, src)));
  const duration = shown.duration;
  const track = timeline && selected ? timeline.tracks.find((t) => t.id === selected.track) : undefined;
  const clip = track?.clips.find((c) => c.id === selected?.clip);
  const drafts = timeline ? draftCount(timeline) : 0;
  const hasVoice = timeline ? spokenCount(timeline) : 0;
  const overlays = timeline ? overlayCount(timeline) : 0;

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

  // Clips on audio and overlay lanes move and trim by pointer; video clips
  // only select (the export always plays the recording whole). Edges snap to
  // other clips and the playhead within SNAP_PX.
  const beginDrag = (e: ReactPointerEvent<HTMLElement>, kind: "move" | "start" | "end", tr: Track, c: Clip) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    setSelected({ track: tr.id, clip: c.id });
    if (tr.kind === "video") return;
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
      ? `Redo the draft clips in ${name} with my cloned voice. ${sourceAudioInstruction(mode)}`
      : `Add my cloned voice to ${source ?? "the video in this project"}: write ${name || "<stem>.timeline.json"} and make the audio for every clip. ${sourceAudioInstruction(mode)}`;
    promptProject(project, prompt).catch((e) => setError(String(e)));
  };

  // Regenerate one clip's voice with its current text: mark it draft, save
  // right away (the agent reads the file), then ask for just that clip.
  const redoClip = (trackId: string, c: Clip) => {
    if (!timeline || !name) return;
    const next = setClipText(timeline, trackId, c.id, c.text ?? "");
    dirtyRef.current = false;
    setTimeline(next);
    api
      .writeTimeline(project, name, serializeTimeline(next))
      .then(() =>
        promptProject(
          project,
          `Redo only the voice of clip ${c.id} in ${name} with my cloned voice, using its current text. Leave every other clip untouched.`,
        ),
      )
      .catch((e) => setError(String(e)));
  };

  // Finder drops reach the page as File objects without a path, so the bytes
  // are copied into the project through the import command.
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

  // Leaves source_audio to the skill: transcribe when whisper finds speech,
  // otherwise mute and narrate the keyframes. Asserting "I am talking" made
  // the agent chase silent tracks.
  const addVoiceTo = (video: string) => {
    const target =
      timeline && source === video && name
        ? name
        : `${video.replace(/^media\//, "").replace(VIDEO_EXT, "")}.timeline.json`;
    const prompt = `Add my cloned voice to ${video}: write ${target} and make the audio for every clip. Set "source_audio" yourself: "transcribe" only if the recording has speech that whisper can transcribe, otherwise "mute" and narrate what happens on screen from the keyframes. Never boost, filter or retry the audio.`;
    promptProject(project, prompt).catch((e) => setError(String(e)));
  };

  const laneAccepts = (tr: Track, file: string | null) =>
    !!file && (VIDEO_EXT.test(file) ? tr.kind !== "audio" : tr.kind === "audio");

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
              disabled={!videoSrc}
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
              <label
                className="flex items-center gap-1 text-[0.72rem] text-muted-foreground"
                title="What happens to the recording's own soundtrack: the agent transcribes it and re-voices it with your clone, it is dropped, or it plays under the voice clips"
              >
                Recording audio
                <select
                  aria-label="Recording audio"
                  value={timeline.source_audio ?? "mute"}
                  onChange={(e) => update({ ...timeline, source_audio: e.target.value as SourceAudio })}
                  className="h-8 max-w-[200px] rounded-md border border-input bg-transparent px-1 text-[0.78rem] text-foreground"
                >
                  {SOURCE_AUDIO.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
              </label>
              <Button variant="outline" size="sm" onClick={() => update(addMarker(timeline, time))}>
                <Plus size={14} /> Add marker
              </Button>
              <Button size="sm" onClick={generate} disabled={running > 0}>
                <Sparkles size={14} /> {hasVoice ? "Redo drafts" : "Add voice"}
              </Button>
              <Button
                variant="outline"
                size="sm"
                title="Render the timeline to an MP4 with ffmpeg"
                onClick={exportVideo}
                disabled={exporting || running > 0 || (hasVoice === 0 && overlays === 0)}
              >
                <FolderOpen size={14} /> {exporting ? "Exporting..." : "Export"}
              </Button>
            </>
          )}
        </div>
      </div>

      <div className="flex flex-col gap-3 p-4">
        {loadError && <p className="text-[0.8rem] text-destructive">{loadError}</p>}

        <div className="flex max-h-[50vh] min-h-[200px] w-full items-center justify-center overflow-hidden rounded-lg bg-black">
          {timeline && videoSrc ? (
            <div className="relative max-h-[50vh] max-w-full">
              <video
                ref={videoRef}
                key={videoSrc}
                src={videoSrc}
                controls
                muted={timeline.source_audio !== "keep"}
                onTimeUpdate={(e) => {
                  setTime(e.currentTarget.currentTime);
                  syncMedia(e.currentTarget.currentTime, !e.currentTarget.paused);
                }}
                onPlay={(e) => {
                  setPlaying(true);
                  syncMedia(e.currentTarget.currentTime, true);
                }}
                onPause={(e) => {
                  setPlaying(false);
                  syncMedia(e.currentTarget.currentTime, false);
                }}
                onEnded={() => setPlaying(false)}
                onSeeked={(e) => syncMedia(e.currentTarget.currentTime, !e.currentTarget.paused)}
                onError={() => setLoadError(`Cannot play ${videoPath}`)}
                className="max-h-[50vh] max-w-full"
              />
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
            </div>
          ) : timeline && videoPath ? (
            <p className="p-6 text-center text-[0.8rem] text-muted-foreground">
              Preview unavailable for {videoPath} (only files under the default projects root can be previewed).
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
                    <select
                      aria-label={`Voice sample for ${tr.id}`}
                      title="The voice sample this track's speech is cloned from (recorded in Settings > Voice samples)"
                      value={samples.some((v) => v.name === tr.voice_sample) ? tr.voice_sample : ""}
                      onChange={(e) =>
                        update({
                          ...shown,
                          tracks: shown.tracks.map((t) =>
                            t.id === tr.id ? { ...t, voice_sample: e.target.value || undefined } : t,
                          ),
                        })
                      }
                      className="h-5 w-full truncate rounded border border-zinc-700 bg-zinc-900 px-1 text-[0.65rem] font-normal text-zinc-300"
                    >
                      <option value="">{tr.voice_sample ? `Voice: ${tr.voice_sample}` : "Voice: agent picks"}</option>
                      {samples.map((v) => (
                        <option key={v.name} value={v.name}>
                          Voice: {v.name}
                        </option>
                      ))}
                    </select>
                  )}
                </div>
                <TrackIcon kind={tr.kind} />
              </div>
            ))}
            <div className="flex h-8 items-center gap-1 px-1.5">
              {(["video", "audio", "overlay"] as TrackKind[]).map((k) => (
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
                    const editable = tr.kind !== "video";
                    const layout = clipLayout(c, pps);
                    const mediaSrc = c.src
                      ? tr.kind === "audio"
                        ? clipSrc(dir, c.src)
                        : safeProjectMediaSrc(resolveSrc(dir, c.src))
                      : null;
                    return (
                      <div key={c.id} className="group absolute top-1 bottom-1" style={layout}>
                        <button
                          title={c.text || c.src || c.id}
                          aria-pressed={isSel}
                          onPointerDown={(e) => beginDrag(e, "move", tr, c)}
                          onPointerMove={(e) => dragTo(e, c)}
                          onPointerUp={endDrag}
                          onPointerCancel={endDrag}
                          className={cn(
                            "absolute inset-0 touch-none overflow-hidden rounded-[3px] border text-left text-[0.68rem] text-white/95 outline-none select-none",
                            editable ? "cursor-grab active:cursor-grabbing" : "cursor-default",
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
                          {editable && (
                            <>
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
                            </>
                          )}
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
            onChange={update}
            onRedo={running > 0 ? undefined : () => redoClip(track.id, clip)}
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
  onChange,
  onRedo,
}: {
  track: Track;
  clip: Clip;
  dir: string;
  timeline: Timeline;
  onChange: (t: Timeline) => void;
  onRedo?: () => void;
}) {
  const audio = clip.src && track.kind !== "video" ? safeAudioSrc(resolveSrc(dir, clip.src)) : null;
  const spoken = track.kind === "audio" && isSpoken(clip);
  return (
    <div className="flex flex-col gap-2 rounded-lg border border-border bg-secondary/40 p-3">
      <div className="flex items-center gap-2 text-[0.75rem] text-muted-foreground">
        <span className="font-medium text-foreground">Clip {clip.id}</span>
        <span>
          {fmtTime(clip.start)} - {fmtTime(clip.end)}
        </span>
        {clip.status && <span className="rounded border border-border px-1">{clip.status}</span>}
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
        {track.kind !== "video" && (
          <button
            aria-label={`Delete clip ${clip.id}`}
            title="Delete clip"
            onClick={() => onChange(removeClip(timeline, track.id, clip.id))}
            className={cn("text-muted-foreground hover:text-destructive", !spoken && "ml-auto")}
          >
            <Trash2 size={14} />
          </button>
        )}
      </div>
      {spoken && (
        <textarea
          id={`clip-text-${clip.id}`}
          aria-label={`Voice text for ${clip.id}`}
          rows={3}
          value={clip.text ?? ""}
          placeholder="What should be said here? Leave blank to let the agent suggest it."
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
