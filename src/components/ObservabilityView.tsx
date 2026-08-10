import { useEffect, useState } from "react";
import { ArrowLeft, Eye } from "lucide-react";
import { api, type StoredSpan } from "@/lib/tauri";
import { useDesktop } from "@/store";

function nsToMs(ns: number): string {
  return (ns / 1_000_000).toFixed(1);
}

function groupByTrace(spans: StoredSpan[]): Map<string, StoredSpan[]> {
  const map = new Map<string, StoredSpan[]>();
  for (const s of spans) {
    const list = map.get(s.trace_id) || [];
    list.push(s);
    map.set(s.trace_id, list);
  }
  return map;
}

function traceTokenCount(spans: StoredSpan[]): number {
  let total = 0;
  for (const s of spans) {
    for (const [k, v] of s.attributes) {
      if (k.toLowerCase().includes("token")) {
        total += parseInt(v, 10) || 0;
      }
    }
  }
  return total;
}

export function ObservabilityView() {
  const { setCurrentView } = useDesktop();
  const [traces, setTraces] = useState<StoredSpan[]>([]);
  const [selectedTrace, setSelectedTrace] = useState<string | null>(null);

  useEffect(() => {
    const fetch = () => {
      api.getTraces().then(setTraces).catch(() => {});
    };
    fetch();
    const id = setInterval(fetch, 3000);
    return () => clearInterval(id);
  }, []);

  const byTrace = groupByTrace(traces);
  const traceIds = Array.from(byTrace.keys());
  const selectedSpans = selectedTrace ? byTrace.get(selectedTrace) || [] : [];

  return (
    <div id="observability-view" className="flex min-h-0 flex-1">
      <nav className="flex w-[240px] shrink-0 flex-col gap-1 border-r border-border bg-secondary p-3">
        <button
          onClick={() => setCurrentView("chat")}
          className="mb-2 inline-flex items-center gap-1.5 rounded-md px-2 py-[0.45rem] text-[0.85rem] font-medium text-muted-foreground hover:bg-primary/10 hover:text-foreground"
        >
          <ArrowLeft size={15} /> Back
        </button>
        <span className="mb-1 px-2 text-[0.7rem] font-semibold uppercase tracking-wide text-muted-foreground">
          Traces
        </span>
        <div className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto">
          {traceIds.length === 0 && (
            <p className="px-2 text-[0.8rem] text-muted-foreground">
              No traces yet. Start a session to see spans.
            </p>
          )}
          {traceIds.map((tid) => {
            const spans = byTrace.get(tid)!;
            const first = spans[0];
            const last = spans[spans.length - 1];
            const dur = last.end_time_unix_nano - first.start_time_unix_nano;
            const svc = first.service_name || "unknown";
            const tokens = traceTokenCount(spans);
            return (
              <button
                key={tid}
                aria-pressed={selectedTrace === tid}
                onClick={() => setSelectedTrace(tid)}
                className={
                  "rounded-md px-3 py-[0.5rem] text-left text-[0.8rem] font-medium " +
                  (selectedTrace === tid
                    ? "bg-primary/15 text-foreground"
                    : "text-muted-foreground hover:bg-primary/10 hover:text-foreground")
                }
              >
                <div className="truncate font-semibold">{svc}</div>
                <div className="mt-0.5 text-[0.7rem] opacity-70">
                  {spans.length} spans &middot; {nsToMs(dur)}ms
                  {tokens > 0 && ` · ${tokens}t`}
                </div>
              </button>
            );
          })}
        </div>
      </nav>

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto p-6">
        <div className="mx-auto w-full max-w-[800px]">
          {!selectedTrace && (
            <div className="flex flex-col items-center gap-3 pt-20 text-muted-foreground">
              <Eye size={40} strokeWidth={1} />
              <p className="text-[0.9rem]">Select a trace from the sidebar</p>
            </div>
          )}
          {selectedTrace && (
            <>
              <h2 className="mb-1 text-[1.05rem] font-semibold">
                Trace {selectedTrace.slice(0, 12)}...
              </h2>
              <p className="mb-4 text-[0.8rem] text-muted-foreground">
                {selectedSpans.length} spans
              </p>
              <div className="flex flex-col gap-1">
                {selectedSpans.map((span) => {
                  const dur = span.end_time_unix_nano - span.start_time_unix_nano;
                  const depth = !span.parent_span_id ? 0 : 1;
                  return (
                    <div
                      key={span.span_id}
                      className="rounded border border-border bg-card px-3 py-2 text-[0.8rem]"
                      style={{ marginLeft: depth * 20 }}
                    >
                      <div className="flex items-center justify-between">
                        <span className="font-medium">{span.name}</span>
                        <span className="text-muted-foreground">{nsToMs(dur)}ms</span>
                      </div>
                      {span.attributes.length > 0 && (
                        <div className="mt-1 flex flex-wrap gap-1">
                          {span.attributes.map(([k, v]) => (
                            <span
                              key={k}
                              className="rounded bg-secondary px-1.5 py-0.5 text-[0.65rem] text-muted-foreground"
                            >
                              {k}={v}
                            </span>
                          ))}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
