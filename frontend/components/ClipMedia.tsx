import { useEffect, useRef } from "react";
import { computePeaks, peakColumns } from "@/lib/peaks";

// What a clip shows inside itself: a waveform for audio, frames for video.
// Both draw straight into a canvas sized to the clip; audio is decoded once
// per file and cached, video frames are grabbed from a hidden element (the
// canvas is tainted by the asset origin, which is fine for display only).

const peakCache = new Map<string, Promise<Float32Array>>();
let audioCtx: AudioContext | null = null;

function peaksFor(src: string): Promise<Float32Array> {
  let p = peakCache.get(src);
  if (!p) {
    p = fetch(src)
      .then((r) => r.arrayBuffer())
      .then((buf) => (audioCtx ??= new AudioContext()).decodeAudioData(buf))
      .then((audio) => computePeaks(audio.getChannelData(0), audio.sampleRate));
    peakCache.set(src, p);
    p.catch(() => peakCache.delete(src));
  }
  return p;
}

export function Waveform({
  src,
  offset,
  length,
  width,
  height,
}: {
  src: string;
  offset: number;
  length: number;
  width: number;
  height: number;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const columns = Math.max(1, Math.round(width));
  useEffect(() => {
    let live = true;
    peaksFor(src)
      .then((peaks) => {
        const canvas = ref.current;
        if (!live || !canvas) return;
        const ctx = canvas.getContext("2d");
        if (!ctx) return;
        canvas.width = columns;
        canvas.height = height;
        ctx.clearRect(0, 0, columns, height);
        ctx.fillStyle = "rgba(255,255,255,0.55)";
        const mid = height / 2;
        peakColumns(peaks, offset, length, columns).forEach((v, x) => {
          const h = Math.max(1, v * (height - 2));
          ctx.fillRect(x, mid - h / 2, 1, h);
        });
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [src, offset, length, columns, height]);
  return <canvas ref={ref} aria-hidden="true" className="pointer-events-none absolute inset-0 h-full w-full" />;
}

export function Thumbnails({
  src,
  start,
  length,
  width,
  height,
}: {
  src: string;
  start: number;
  length: number;
  width: number;
  height: number;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const thumbW = Math.round((height * 16) / 9);
  const count = Math.max(1, Math.ceil(width / thumbW));
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    let live = true;
    const video = document.createElement("video");
    video.muted = true;
    video.preload = "auto";
    video.src = src;
    const ctx = canvas.getContext("2d");
    canvas.width = count * thumbW;
    canvas.height = height;
    const grab = (i: number) => {
      if (!live || !ctx || i >= count) return;
      const at = start + ((i + 0.5) / count) * length;
      video.onseeked = () => {
        if (!live) return;
        ctx.drawImage(video, i * thumbW, 0, thumbW, height);
        grab(i + 1);
      };
      video.currentTime = Math.min(at, Math.max(0, video.duration - 0.05));
    };
    video.onloadedmetadata = () => grab(0);
    video.onerror = () => {};
    return () => {
      live = false;
      video.onseeked = null;
      video.removeAttribute("src");
      video.load();
    };
  }, [src, start, length, count, thumbW, height]);
  return (
    <canvas
      ref={ref}
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 h-full w-full opacity-80"
      style={{ objectFit: "cover", objectPosition: "left" }}
    />
  );
}
