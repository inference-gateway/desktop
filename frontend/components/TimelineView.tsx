import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowLeft, Clapperboard, FilePlus, FolderOpen, Plus, RefreshCw, Sparkles, Trash2 } from "lucide-react";
import { api, type ProjectFile } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { safeAudioSrc, safeProjectMediaSrc } from "@/lib/tools";
import {
  addMarker,
  clipLayout,
  draftCount,
  fmtTime,
  voiceTrack,
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
} from "@/lib/timeline";
import { useDesktop } from "@/store";
import { AudioPlayer } from "./AudioPlayer";
import { Button } from "@/components/ui/button";

const SAVE_DEBOUNCE_MS = 600;
const SYNC_TOLERANCE_S = 0.3;

// Clip audio lives either in ~/.infer/tts (voice) or in the project dir (music).
function clipSrc(dir: string, src: string): string | null {
  const path = resolveSrc(dir, src);
  return safeAudioSrc(path) ?? safeProjectMediaSrc(path);
}
const TRACK_LABEL: Record<Track["kind"], string> = { video: "Video", voice: "Voice", audio: "Audio" };
const VIDEO_EXT = /\.(?:mp4|mov|m4v|webm)$/i;

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

// Editable view of <stem>.timeline.json: video preview, one row per track,
// clips positioned by time, and a text editor for the selected voice
// clip. Edits mark clips draft and are debounced to disk; the agent
// synthesizes drafts and remuxes when asked via "Generate".
export function TimelineView() {
  const { timelineProject: project, setCurrentView, promptProject, runningIds, setError } = useDesktop();
  const [dir, setDir] = useState("");
  const [names, setNames] = useState<string[]>([]);
  const [name, setName] = useState("");
  const [timeline, setTimeline] = useState<Timeline | null>(null);
  const [loadError, setLoadError] = useState("");
  const [selected, setSelected] = useState<{ track: string; clip: string } | null>(null);
  const [exporting, setExporting] = useState(false);
  const [time, setTime] = useState(0);
  const [videos, setVideos] = useState<ProjectFile[]>([]);
  const [newSourceAudio, setNewSourceAudio] = useState<SourceAudio>("transcribe");
  const dirtyRef = useRef(false);
  const videoRef = useRef<HTMLVideoElement>(null);
  const audioRefs = useRef(new Map<string, HTMLAudioElement>());
  const { setStatus } = useDesktop();

  // The preview plays whatever the timeline holds: every clip's audio is kept
  // in step with the video's clock; the original sound stays only with "keep".
  const syncAudio = (t: number, playing: boolean) => {
    if (!timeline) return;
    for (const tr of timeline.tracks) {
      if (tr.kind === "video") continue;
      for (const c of tr.clips) {
        const el = audioRefs.current.get(c.id);
        if (!el) continue;
        el.volume = Math.max(0, Math.min(1, tr.gain ?? 1));
        const offset = t - c.start;
        const length = Number.isFinite(el.duration) && el.duration > 0 ? el.duration : c.end - c.start;
        const inside = offset >= 0 && offset < length;
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
          .listProjectFiles(project)
          .then((files) => setVideos(files.filter((f) => VIDEO_EXT.test(f.name))))
          .catch(() => setVideos([]));
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project]);

  // Reload when an agent run finishes, unless local edits are pending.
  const running = runningIds.size;
  useEffect(() => {
    if (!dirtyRef.current) load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [running]);

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
    setTimeline(next);
  };

  const seek = (t: number) => {
    const el = videoRef.current;
    if (el) el.currentTime = t;
    setTime(t);
    syncAudio(t, !!el && !el.paused);
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

  if (!project) return null;

  const source = timeline ? videoSource(timeline) : undefined;
  const videoPath = source;
  const videoSrc = videoPath ? safeProjectMediaSrc(resolveSrc(dir, videoPath)) : null;
  const clipAudio = timeline
    ? timeline.tracks
        .filter((tr) => tr.kind !== "video")
        .flatMap((tr) => tr.clips)
        .flatMap((c) => (c.src ? [{ id: c.id, src: clipSrc(dir, c.src) }] : []))
        .filter((c): c is { id: string; src: string } => !!c.src)
    : [];
  const duration = timeline?.duration ?? 0;
  const track = timeline && selected ? timeline.tracks.find((t) => t.id === selected.track) : undefined;
  const clip = track?.clips.find((c) => c.id === selected?.clip);
  const drafts = timeline ? draftCount(timeline) : 0;
  const hasVoice = (timeline && voiceTrack(timeline)?.clips.length) ?? 0;

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

  const addRecording = () => {
    api
      .addProjectVideo(project)
      .then((added) => {
        if (added) load();
      })
      .catch((e) => setError(String(e)));
  };

  const addVoiceTo = (video: string) => {
    const stem = video.replace(VIDEO_EXT, "");
    const prompt = `Add my cloned voice to ${video}: write ${stem}.timeline.json with "source_audio": "${newSourceAudio}" and make the audio for every clip. ${sourceAudioInstruction(newSourceAudio)}`;
    promptProject(project, prompt).catch((e) => setError(String(e)));
  };

  return (
    <div id="timeline-view" className="flex min-h-0 flex-1">
      <nav className="flex w-[240px] shrink-0 flex-col gap-1 border-r border-border bg-secondary p-3">
        <button
          onClick={() => setCurrentView("chat")}
          className="mb-2 inline-flex items-center gap-1.5 rounded-md px-2 py-[0.45rem] text-[0.85rem] font-medium text-muted-foreground hover:bg-primary/10 hover:text-foreground"
        >
          <ArrowLeft size={15} /> Back
        </button>
        <span className="mb-1 truncate px-2 text-[0.7rem] font-semibold uppercase tracking-wide text-muted-foreground">
          {project}
        </span>
        {names.map((n) => (
          <button
            key={n}
            aria-pressed={n === name}
            onClick={() => load(n)}
            className={cn(
              "inline-flex items-center gap-2 truncate rounded-md px-3 py-[0.5rem] text-left text-[0.85rem] font-medium",
              n === name
                ? "bg-primary/15 text-foreground"
                : "text-muted-foreground hover:bg-primary/10 hover:text-foreground",
            )}
          >
            <Clapperboard size={14} className="shrink-0" />
            {n.replace(/\.timeline\.json$/, "")}
          </button>
        ))}
        {names.length === 0 && (
          <p className="px-2 text-[0.78rem] text-muted-foreground">No timeline yet. Add a recording to get started.</p>
        )}
      </nav>

      <div className="flex min-w-0 flex-1 flex-col gap-3 overflow-y-auto p-4">
        {loadError && <p className="text-[0.8rem] text-destructive">{loadError}</p>}
        {!timeline && (
          <div className="flex flex-col gap-2 rounded-lg border border-border p-3">
            <div className="flex items-center gap-2">
              <h2 className="text-[0.9rem] font-semibold">Recordings in this project</h2>
              <Button variant="outline" size="sm" className="ml-auto" onClick={addRecording}>
                <FilePlus size={14} /> Add recording
              </Button>
            </div>
            {videos.length === 0 && (
              <p className="text-[0.8rem] text-muted-foreground">No recordings yet. Add a .mov or .mp4 file.</p>
            )}
            <label className="flex items-center gap-2 text-[0.78rem] text-muted-foreground">
              Original audio
              <select
                aria-label="Original audio"
                value={newSourceAudio}
                onChange={(e) => setNewSourceAudio(e.target.value as SourceAudio)}
                className="h-7 rounded-md border border-input bg-transparent px-1 text-[0.78rem] text-foreground"
              >
                {SOURCE_AUDIO.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </select>
            </label>
            {videos.map((v) => (
              <div key={v.name} className="flex items-center gap-2 text-[0.85rem]">
                <span className="truncate">{v.name}</span>
                <Button size="sm" className="ml-auto" disabled={running > 0} onClick={() => addVoiceTo(v.name)}>
                  <Sparkles size={14} /> Add voice
                </Button>
              </div>
            ))}
          </div>
        )}
        {timeline && (
          <>
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="text-[0.95rem] font-semibold">{name.replace(/\.timeline\.json$/, "")}</h2>
              <span className="text-[0.75rem] text-muted-foreground">
                {fmtTime(time)} / {fmtTime(duration)}
                {drafts > 0 && ` · ${drafts} draft${drafts === 1 ? "" : "s"}`}
              </span>
              <div className="ml-auto flex items-center gap-1.5">
                <select
                  aria-label="Original audio"
                  title="What to do with the recording's own audio"
                  value={timeline.source_audio ?? "mute"}
                  onChange={(e) => update({ ...timeline, source_audio: e.target.value as SourceAudio })}
                  className="h-8 rounded-md border border-input bg-transparent px-1 text-[0.78rem] text-foreground"
                >
                  {SOURCE_AUDIO.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
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
                  disabled={exporting || running > 0 || hasVoice === 0}
                >
                  <FolderOpen size={14} /> {exporting ? "Exporting..." : "Export"}
                </Button>
              </div>
            </div>

            {videoSrc ? (
              <video
                ref={videoRef}
                key={videoSrc}
                src={videoSrc}
                controls
                muted={timeline.source_audio !== "keep"}
                onTimeUpdate={(e) => {
                  setTime(e.currentTarget.currentTime);
                  syncAudio(e.currentTarget.currentTime, !e.currentTarget.paused);
                }}
                onPlay={(e) => syncAudio(e.currentTarget.currentTime, true)}
                onPause={(e) => syncAudio(e.currentTarget.currentTime, false)}
                onSeeked={(e) => syncAudio(e.currentTarget.currentTime, !e.currentTarget.paused)}
                onError={() => setLoadError(`Cannot play ${videoPath}`)}
                className="max-h-[45vh] w-full rounded-lg bg-black"
              />
            ) : null}
            {clipAudio.map((c) => (
              <audio
                key={c.id}
                src={c.src}
                preload="auto"
                ref={(el) => {
                  if (el) audioRefs.current.set(c.id, el);
                  else audioRefs.current.delete(c.id);
                }}
              />
            ))}
            {videoSrc ? null : (
              <p className="rounded-lg border border-border p-3 text-[0.8rem] text-muted-foreground">
                Preview unavailable for {videoPath ?? "this timeline"} (only files under the default projects root can
                be previewed).
              </p>
            )}

            <div className="flex flex-col gap-1.5">
              <div
                className="relative h-4 cursor-pointer text-[0.65rem] text-muted-foreground"
                onClick={(e) => {
                  const r = e.currentTarget.getBoundingClientRect();
                  seek(((e.clientX - r.left) / r.width) * duration);
                }}
              >
                {[0, 0.25, 0.5, 0.75, 1].map((f) => (
                  <span key={f} className="absolute -translate-x-1/2" style={{ left: `${f * 100}%` }}>
                    {fmtTime(f * duration)}
                  </span>
                ))}
              </div>
              {timeline.tracks.map((tr) => (
                <div key={tr.id} className="flex items-center gap-2">
                  <span className="w-20 shrink-0 text-[0.72rem] text-muted-foreground">{TRACK_LABEL[tr.kind]}</span>
                  <div className="relative h-8 flex-1 rounded-md bg-secondary">
                    {tr.clips.map((c) => (
                      <button
                        key={c.id}
                        title={c.text || c.src || c.id}
                        aria-pressed={selected?.track === tr.id && selected.clip === c.id}
                        onClick={() => {
                          setSelected({ track: tr.id, clip: c.id });
                          seek(c.start);
                        }}
                        style={clipLayout(c, duration)}
                        className={cn(
                          "absolute top-0.5 bottom-0.5 truncate rounded px-1 text-left text-[0.68rem] leading-7 outline-none",
                          tr.kind === "video" && "bg-primary/30",
                          tr.kind === "audio" && "bg-emerald-500/30",
                          tr.kind === "voice" && (c.status === "draft" ? "bg-amber-500/40" : "bg-primary/50"),
                          selected?.track === tr.id && selected.clip === c.id && "ring-2 ring-ring",
                        )}
                      >
                        {c.text || c.id}
                      </button>
                    ))}
                    {duration > 0 && (
                      <div
                        className="pointer-events-none absolute top-0 bottom-0 w-px bg-foreground"
                        style={{ left: `${(time / duration) * 100}%` }}
                      />
                    )}
                  </div>
                </div>
              ))}
            </div>

            {track && clip && (
              <ClipEditor
                track={track}
                clip={clip}
                dir={dir}
                timeline={timeline}
                onChange={update}
                onRedo={running > 0 ? undefined : () => redoClip(track.id, clip)}
              />
            )}
          </>
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
  return (
    <div className="flex flex-col gap-2 rounded-lg border border-border p-3">
      <div className="flex items-center gap-2 text-[0.75rem] text-muted-foreground">
        <span className="font-medium text-foreground">{clip.id}</span>
        <span>
          {fmtTime(clip.start)} - {fmtTime(clip.end)}
        </span>
        {clip.status && <span className="rounded border border-border px-1">{clip.status}</span>}
        {track.kind === "voice" && (
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
            className={cn("text-muted-foreground hover:text-destructive", track.kind !== "voice" && "ml-auto")}
          >
            <Trash2 size={14} />
          </button>
        )}
      </div>
      {track.kind === "voice" && (
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
