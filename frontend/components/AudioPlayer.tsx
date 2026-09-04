import { useEffect, useRef, useState } from "react";
import { Check, Download, Loader2, Pause, Play, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { computePeaks } from "@/lib/audio";
import { api } from "@/lib/tauri";

// WhatsApp-style voice message bubble: play/pause, a real waveform decoded
// from the WAV (flat bars until decoding finishes or if it fails), a playhead
// line over played-vs-unplayed bar coloring, click to seek, elapsed/total
// time, and a hover download button (copies to ~/Downloads via save_audio).
// Bar heights are set in px: percentage heights collapse to 0 in WKWebView.
const BARS = 40;
const BAR_MAX_PX = 26;
const FLAT = Array<number>(BARS).fill(0.3);

function fmt(t: number): string {
  if (!isFinite(t)) return "0:00";
  const s = Math.floor(t);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export function AudioPlayer({ src, ariaLabel, path }: { src: string; ariaLabel: string; path?: string }) {
  const audioRef = useRef<HTMLAudioElement>(null);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [peaks, setPeaks] = useState<number[]>(FLAT);
  const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const buf = await (await fetch(src)).arrayBuffer();
        const Ctx: typeof AudioContext =
          window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
        const ctx = new Ctx();
        const decoded = await ctx.decodeAudioData(buf);
        if (alive) {
          setPeaks(computePeaks(decoded.getChannelData(0), BARS));
          setDuration(decoded.duration);
        }
        await ctx.close();
      } catch {
        // Keep the flat waveform - playback still works via the <audio> element.
      }
    })();
    return () => {
      alive = false;
    };
  }, [src]);

  useEffect(() => {
    if (saveStatus !== "saved" && saveStatus !== "error") return;
    const t = setTimeout(() => setSaveStatus("idle"), 2000);
    return () => clearTimeout(t);
  }, [saveStatus]);

  const toggle = () => {
    const el = audioRef.current;
    if (!el) return;
    if (el.paused) void el.play();
    else el.pause();
  };

  const seek = (e: React.MouseEvent<HTMLDivElement>) => {
    const el = audioRef.current;
    if (!el || !duration) return;
    const rect = e.currentTarget.getBoundingClientRect();
    el.currentTime = ((e.clientX - rect.left) / rect.width) * duration;
    setTime(el.currentTime);
  };

  const save = () => {
    if (!path || saveStatus === "saving") return;
    setSaveStatus("saving");
    api
      .saveAudio(path)
      .then(() => setSaveStatus("saved"))
      .catch(() => setSaveStatus("error"));
  };

  const progress = duration > 0 ? time / duration : 0;
  const SaveIcon =
    saveStatus === "saving" ? Loader2 : saveStatus === "saved" ? Check : saveStatus === "error" ? X : Download;

  return (
    <div className="group flex w-[26rem] max-w-full items-center gap-2 rounded-full border border-border bg-card py-2 pr-4 pl-2">
      <audio
        ref={audioRef}
        preload="auto"
        src={src}
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onEnded={() => setPlaying(false)}
        onTimeUpdate={(e) => setTime(e.currentTarget.currentTime)}
        onLoadedMetadata={(e) => {
          if (isFinite(e.currentTarget.duration)) setDuration(e.currentTarget.duration);
        }}
      />
      <button
        aria-label={playing ? `Pause ${ariaLabel}` : `Play ${ariaLabel}`}
        onClick={toggle}
        className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground hover:opacity-90"
      >
        {playing ? <Pause size={15} /> : <Play size={15} className="translate-x-px" />}
      </button>
      <div
        role="slider"
        aria-label={`Seek ${ariaLabel}`}
        aria-valuemin={0}
        aria-valuemax={Math.round(duration)}
        aria-valuenow={Math.round(time)}
        onClick={seek}
        className="relative flex h-8 flex-1 cursor-pointer items-center gap-px"
      >
        {peaks.map((p, i) => (
          <div
            key={i}
            className={cn(
              "flex-1 rounded-full",
              i / peaks.length <= progress ? "bg-primary" : "bg-muted-foreground/50",
            )}
            style={{ height: `${Math.max(4, Math.round(p * BAR_MAX_PX))}px` }}
          />
        ))}
        {(playing || time > 0) && (
          <div
            className="pointer-events-none absolute top-0 bottom-0 w-0.5 rounded-full bg-primary"
            style={{ left: `${progress * 100}%` }}
          />
        )}
      </div>
      <span className="shrink-0 text-[0.7rem] tabular-nums text-muted-foreground">
        {playing || time > 0 ? `${fmt(time)} / ${fmt(duration)}` : fmt(duration)}
      </span>
      {path && (
        <button
          aria-label={`Download ${ariaLabel}`}
          title={saveStatus === "saved" ? "Saved to Downloads" : "Save to Downloads"}
          onClick={save}
          disabled={saveStatus === "saving"}
          className={cn(
            "inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100 hover:bg-secondary hover:text-foreground focus-visible:opacity-100",
            saveStatus === "saving" && "opacity-100",
            saveStatus === "saved" && "text-green-600 opacity-100 dark:text-green-500",
            saveStatus === "error" && "text-destructive opacity-100",
          )}
        >
          <SaveIcon size={13} className={cn(saveStatus === "saving" && "animate-spin")} />
        </button>
      )}
    </div>
  );
}
