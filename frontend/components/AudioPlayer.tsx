import { useEffect, useRef, useState } from "react";
import { Pause, Play } from "lucide-react";
import { cn } from "@/lib/utils";
import { computePeaks } from "@/lib/audio";

// WhatsApp-style voice message bubble: play/pause, a real waveform decoded
// from the WAV (flat bars until decoding finishes or if it fails), click to
// seek, and elapsed/total time. Replaces the native <audio controls>, which
// WKWebView renders as a bare play button.
const BARS = 40;
const FLAT = Array<number>(BARS).fill(0.25);

function fmt(t: number): string {
  if (!isFinite(t)) return "0:00";
  const s = Math.floor(t);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export function AudioPlayer({ src, ariaLabel }: { src: string; ariaLabel: string }) {
  const audioRef = useRef<HTMLAudioElement>(null);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [peaks, setPeaks] = useState<number[]>(FLAT);

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
  };

  const progress = duration > 0 ? time / duration : 0;

  return (
    <div className="flex w-full max-w-80 items-center gap-2 rounded-full border border-border bg-card py-2 pr-4 pl-2">
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
        className="flex h-8 flex-1 cursor-pointer items-center gap-px"
      >
        {peaks.map((p, i) => (
          <div
            key={i}
            className={cn(
              "min-h-[3px] flex-1 rounded-full transition-colors",
              i / peaks.length <= progress ? "bg-primary" : "bg-muted-foreground/40",
            )}
            style={{ height: `${Math.max(10, p * 100)}%` }}
          />
        ))}
      </div>
      <span className="shrink-0 text-[0.7rem] tabular-nums text-muted-foreground">
        {fmt(playing || time > 0 ? time : duration)}
      </span>
    </div>
  );
}
