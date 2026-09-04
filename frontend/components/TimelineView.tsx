import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowLeft, Clapperboard, FolderOpen, Plus, Sparkles, Trash2 } from "lucide-react";
import { api } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { safeAudioSrc, safeVideoSrc } from "@/lib/tools";
import {
  addMarker,
  clipLayout,
  draftCount,
  fmtTime,
  narrationTrack,
  parseTimeline,
  removeClip,
  resolveSrc,
  serializeTimeline,
  setClipText,
  videoSource,
  type Clip,
  type Timeline,
  type Track,
} from "@/lib/timeline";
import { useDesktop } from "@/store";
import { AudioPlayer } from "./AudioPlayer";
import { Button } from "@/components/ui/button";

const SAVE_DEBOUNCE_MS = 600;
const TRACK_LABEL: Record<Track["kind"], string> = { video: "Video", narration: "Narration", audio: "Audio" };

// Editable view of <stem>.timeline.json: video preview, one row per track,
// clips positioned by time, and a text editor for the selected narration
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
  const [showOutput, setShowOutput] = useState(false);
  const [time, setTime] = useState(0);
  const dirtyRef = useRef(false);
  const videoRef = useRef<HTMLVideoElement>(null);

  const load = useCallback(
    async (pick?: string) => {
      if (!project) return;
      try {
        const list = await api.listTimelines(project);
        setDir(list.dir);
        setNames(list.names);
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
  };

  if (!project) return null;

  const source = timeline ? videoSource(timeline) : undefined;
  const videoPath = timeline && (showOutput && timeline.output ? timeline.output : source);
  const videoSrc = videoPath ? safeVideoSrc(resolveSrc(dir, videoPath)) : null;
  const duration = timeline?.duration ?? 0;
  const track = timeline && selected ? timeline.tracks.find((t) => t.id === selected.track) : undefined;
  const clip = track?.clips.find((c) => c.id === selected?.clip);
  const drafts = timeline ? draftCount(timeline) : 0;
  const hasNarration = (timeline && narrationTrack(timeline)?.clips.length) ?? 0;

  const generate = () => {
    const prompt = hasNarration
      ? `Regenerate the draft clips in ${name} with my cloned voice and remux the video.`
      : `Narrate ${source ?? "the video in this project"} in my cloned voice: write ${name || "<stem>.timeline.json"}, synthesize every clip and mux the result.`;
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
          <p className="px-2 text-[0.78rem] text-muted-foreground">
            No timeline yet. Drop a recording into this project and ask the agent to narrate it.
          </p>
        )}
      </nav>

      <div className="flex min-w-0 flex-1 flex-col gap-3 overflow-y-auto p-4">
        {loadError && <p className="text-[0.8rem] text-destructive">{loadError}</p>}
        {timeline && (
          <>
            <div className="flex flex-wrap items-center gap-2">
              <h2 className="text-[0.95rem] font-semibold">{name.replace(/\.timeline\.json$/, "")}</h2>
              <span className="text-[0.75rem] text-muted-foreground">
                {fmtTime(time)} / {fmtTime(duration)}
                {drafts > 0 && ` · ${drafts} draft${drafts === 1 ? "" : "s"}`}
              </span>
              <div className="ml-auto flex items-center gap-1.5">
                {timeline.output && (
                  <Button
                    variant="outline"
                    size="sm"
                    aria-pressed={showOutput}
                    onClick={() => setShowOutput((v) => !v)}
                  >
                    {showOutput ? "Original" : "Narrated"}
                  </Button>
                )}
                <Button variant="outline" size="sm" onClick={() => update(addMarker(timeline, time))}>
                  <Plus size={14} /> Add marker
                </Button>
                <Button size="sm" onClick={generate} disabled={running > 0}>
                  <Sparkles size={14} /> {hasNarration ? "Regenerate drafts" : "Generate narration"}
                </Button>
                {timeline.output && (
                  <Button
                    variant="outline"
                    size="sm"
                    title="Reveal the exported video"
                    onClick={() => api.revealProjectFile(project, timeline.output!).catch((e) => setError(String(e)))}
                  >
                    <FolderOpen size={14} /> Export
                  </Button>
                )}
              </div>
            </div>

            {videoSrc ? (
              <video
                ref={videoRef}
                key={videoSrc}
                src={videoSrc}
                controls
                onTimeUpdate={(e) => setTime(e.currentTarget.currentTime)}
                onError={() => setLoadError(`Cannot play ${videoPath}`)}
                className="max-h-[45vh] w-full rounded-lg bg-black"
              />
            ) : (
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
                          tr.kind === "narration" && (c.status === "draft" ? "bg-amber-500/40" : "bg-primary/50"),
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

            {track && clip && <ClipEditor track={track} clip={clip} dir={dir} timeline={timeline} onChange={update} />}
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
}: {
  track: Track;
  clip: Clip;
  dir: string;
  timeline: Timeline;
  onChange: (t: Timeline) => void;
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
        {track.kind !== "video" && (
          <button
            aria-label={`Delete clip ${clip.id}`}
            title="Delete clip"
            onClick={() => onChange(removeClip(timeline, track.id, clip.id))}
            className="ml-auto text-muted-foreground hover:text-destructive"
          >
            <Trash2 size={14} />
          </button>
        )}
      </div>
      {track.kind === "narration" && (
        <textarea
          id={`clip-text-${clip.id}`}
          aria-label={`Narration text for ${clip.id}`}
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
