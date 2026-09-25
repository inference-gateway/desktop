// Live view of the active session's agent recording (RecordStart): the area it
// captures and every key press and click since it started, newest last. The
// backend saves the same log next to the MP4 as <name>.events.jsonl.
import { useEffect, useRef } from "react";
import { formatInput } from "@/lib/recording";
import { useDesktop } from "@/store";

export function RecordingPanel() {
  const { recording } = useDesktop();
  const listRef = useRef<HTMLUListElement>(null);
  const lines = (recording?.inputs ?? []).map(formatInput).filter((line) => line !== null);

  useEffect(() => {
    listRef.current?.scrollTo({ top: listRef.current.scrollHeight });
  }, [lines.length]);

  if (!recording) return null;
  const area = recording.area;
  return (
    <div className="mx-5 mb-1 shrink-0 overflow-hidden rounded-md border border-destructive/40 bg-tool-bg text-[0.85rem]">
      <div aria-live="polite" className="flex items-center gap-2 px-3 py-[0.35rem] font-bold text-destructive">
        <span className="h-2 w-2 animate-pulse rounded-full bg-destructive" aria-hidden="true" />
        Recording the screen
        {area && (
          <span className="font-normal text-muted-foreground">
            {area.width}×{area.height} at ({area.x}, {area.y})
          </span>
        )}
      </div>
      {lines.length > 0 && (
        <ul
          ref={listRef}
          aria-label="Recorded input"
          className="max-h-28 overflow-y-auto border-t border-tool-border px-3 py-1 font-mono text-[0.78rem] text-muted-foreground"
        >
          {lines.map((line, i) => (
            <li key={i}>{line}</li>
          ))}
        </ul>
      )}
    </div>
  );
}
