import { useEffect, useState } from "react";
import { ArrowLeft, ChartColumn, Gauge } from "lucide-react";
import { api, type StoredMetric, type StoredSpan } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";

const SERVICE_COLORS = ["#4f8ef7", "#2fbf71", "#f2a93b", "#e05d7e", "#9b6ef3", "#3ec3c9"];

type Section = "traces" | "stats";

const SECTIONS: { id: Section; label: string; icon: typeof ChartColumn }[] = [
  { id: "traces", label: "Traces", icon: ChartColumn },
  { id: "stats", label: "Stats", icon: Gauge },
];

function fmtNs(ns: number): string {
  const ms = ns / 1_000_000;
  if (ms >= 1000) return `${(ms / 1000).toFixed(2)}s`;
  if (ms >= 1) return `${ms.toFixed(1)}ms`;
  return `${(ns / 1000).toFixed(0)}µs`;
}

function fmtCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 10_000) return `${(n / 1000).toFixed(1)}k`;
  return Number.isInteger(n) ? n.toLocaleString() : n.toFixed(2);
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

function traceWindow(spans: StoredSpan[]): { start: number; end: number } {
  let start = Infinity;
  let end = 0;
  for (const s of spans) {
    if (s.start_time_unix_nano < start) start = s.start_time_unix_nano;
    if (s.end_time_unix_nano > end) end = s.end_time_unix_nano;
  }
  return { start, end };
}

// Flatten spans into a depth-first tree ordered by start time. Spans whose
// parent is missing from the trace (still in flight, dropped) act as roots.
function spanTree(spans: StoredSpan[]): { span: StoredSpan; depth: number }[] {
  const ids = new Set(spans.map((s) => s.span_id));
  const children = new Map<string, StoredSpan[]>();
  const roots: StoredSpan[] = [];
  const sorted = [...spans].sort((a, b) => a.start_time_unix_nano - b.start_time_unix_nano);
  for (const s of sorted) {
    if (s.parent_span_id && ids.has(s.parent_span_id)) {
      const list = children.get(s.parent_span_id) || [];
      list.push(s);
      children.set(s.parent_span_id, list);
    } else {
      roots.push(s);
    }
  }
  const out: { span: StoredSpan; depth: number }[] = [];
  const visit = (s: StoredSpan, depth: number) => {
    out.push({ span: s, depth });
    for (const c of children.get(s.span_id) || []) visit(c, depth + 1);
  };
  for (const r of roots) visit(r, 0);
  return out;
}

function serviceColors(spans: StoredSpan[]): Map<string, string> {
  const map = new Map<string, string>();
  for (const s of spans) {
    const svc = s.service_name || "unknown";
    if (!map.has(svc)) map.set(svc, SERVICE_COLORS[map.size % SERVICE_COLORS.length]);
  }
  return map;
}

// The CLI exports delta temporality, so the total for a metric is the sum of
// its stored points. `sumWhere` filters by attribute when the caller needs a
// single series (e.g. input vs output tokens).
function sumWhere(metrics: StoredMetric[], name: string, attr?: [string, string]): number {
  let total = 0;
  for (const m of metrics) {
    if (m.name !== name) continue;
    if (attr && !m.attributes.some(([k, v]) => k === attr[0] && v === attr[1])) continue;
    total += m.value;
  }
  return total;
}

type Aggregate = { name: string; unit: string; value: number; count: number; points: number };

function aggregateByName(metrics: StoredMetric[]): Aggregate[] {
  const map = new Map<string, Aggregate>();
  for (const m of metrics) {
    const agg = map.get(m.name) || { name: m.name, unit: m.unit, value: 0, count: 0, points: 0 };
    agg.value += m.value;
    agg.count += m.count;
    agg.points += 1;
    map.set(m.name, agg);
  }
  return Array.from(map.values()).sort((a, b) => a.name.localeCompare(b.name));
}

function StatTile({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="rounded-md border border-border bg-card px-4 py-3">
      <div className="text-[0.7rem] font-medium uppercase tracking-wide text-muted-foreground">{label}</div>
      <div className="mt-1 text-[1.3rem] font-semibold tabular-nums">{value}</div>
      {hint && <div className="text-[0.7rem] text-muted-foreground">{hint}</div>}
    </div>
  );
}

function StatsSection({ metrics }: { metrics: StoredMetric[] }) {
  const inputTokens = sumWhere(metrics, "gen_ai.client.token.usage", ["gen_ai.token.type", "input"]);
  const outputTokens = sumWhere(metrics, "gen_ai.client.token.usage", ["gen_ai.token.type", "output"]);
  const allTokens = sumWhere(metrics, "gen_ai.client.token.usage");
  const cost = sumWhere(metrics, "infer.client.cost");
  const toolCalls = sumWhere(metrics, "infer.agent.tool.calls");
  const runs = sumWhere(metrics, "infer.agent.runs");
  const aggregates = aggregateByName(metrics);

  if (metrics.length === 0) {
    return (
      <div className="flex flex-col items-center gap-3 pt-20 text-muted-foreground">
        <Gauge size={40} strokeWidth={1} />
        <p className="text-[0.9rem]">No stats yet. Start a session to collect metrics.</p>
      </div>
    );
  }

  return (
    <>
      <h2 className="mb-3 text-[1.05rem] font-semibold">Stats</h2>
      <div className="mb-5 grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-3">
        <StatTile
          label="Tokens"
          value={fmtCount(allTokens)}
          hint={`in ${fmtCount(inputTokens)} · out ${fmtCount(outputTokens)}`}
        />
        <StatTile label="Cost" value={`$${cost.toFixed(4)}`} />
        <StatTile label="Tool calls" value={fmtCount(toolCalls)} />
        <StatTile label="Agent runs" value={fmtCount(runs)} />
      </div>

      <div className="rounded-md border border-border bg-card">
        <div className="flex border-b border-border px-3 py-1.5 text-[0.65rem] font-medium uppercase tracking-wide text-muted-foreground">
          <span className="flex-1">Metric</span>
          <span className="w-24 text-right">Total</span>
          <span className="w-24 text-right">Points</span>
          <span className="w-20 text-right">Unit</span>
        </div>
        {aggregates.map((a) => (
          <div
            key={a.name}
            className="flex border-b border-border/50 px-3 py-1.5 text-[0.75rem] last:border-b-0"
          >
            <span className="flex-1 truncate font-mono">{a.name}</span>
            <span className="w-24 text-right tabular-nums">{fmtCount(a.value)}</span>
            <span className="w-24 text-right tabular-nums text-muted-foreground">{a.points}</span>
            <span className="w-20 truncate text-right text-muted-foreground">{a.unit}</span>
          </div>
        ))}
      </div>
    </>
  );
}

export function ObservabilityView() {
  const { setCurrentView } = useDesktop();
  const [section, setSection] = useState<Section>("traces");
  const [traces, setTraces] = useState<StoredSpan[]>([]);
  const [metrics, setMetrics] = useState<StoredMetric[]>([]);
  const [selectedTrace, setSelectedTrace] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());

  useEffect(() => {
    const fetch = () => {
      api.getTraces().then(setTraces).catch(() => {});
      api.getMetrics().then(setMetrics).catch(() => {});
    };
    fetch();
    const id = setInterval(fetch, 3000);
    return () => clearInterval(id);
  }, []);

  const byTrace = groupByTrace(traces);
  const traceIds = Array.from(byTrace.keys());
  const selectedSpans = selectedTrace ? byTrace.get(selectedTrace) || [] : [];
  const colors = serviceColors(selectedSpans);
  const window = traceWindow(selectedSpans);
  const total = Math.max(window.end - window.start, 1);
  const rows = spanTree(selectedSpans);

  const toggle = (id: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

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
          Observability
        </span>
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            aria-pressed={section === s.id}
            onClick={() => setSection(s.id)}
            className={cn(
              "inline-flex items-center gap-2 rounded-md px-3 py-[0.5rem] text-left text-[0.85rem] font-medium",
              section === s.id
                ? "bg-primary/15 text-foreground"
                : "text-muted-foreground hover:bg-primary/10 hover:text-foreground",
            )}
          >
            <s.icon size={15} /> {s.label}
          </button>
        ))}
        {section === "traces" && (
          <>
            <span className="mb-1 mt-3 px-2 text-[0.7rem] font-semibold uppercase tracking-wide text-muted-foreground">
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
                const w = traceWindow(spans);
                const svc = spans[0].service_name || "unknown";
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
                      {spans.length} spans &middot; {fmtNs(w.end - w.start)}
                      {tokens > 0 && ` · ${tokens}t`}
                    </div>
                  </button>
                );
              })}
            </div>
          </>
        )}
      </nav>

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto p-6">
        <div className="mx-auto w-full max-w-[1100px]">
          {section === "stats" && <StatsSection metrics={metrics} />}
          {section === "traces" && !selectedTrace && (
            <div className="flex flex-col items-center gap-3 pt-20 text-muted-foreground">
              <ChartColumn size={40} strokeWidth={1} />
              <p className="text-[0.9rem]">Select a trace from the sidebar</p>
            </div>
          )}
          {section === "traces" && selectedTrace && (
            <>
              <div className="mb-1 flex items-baseline gap-3">
                <h2 className="text-[1.05rem] font-semibold">
                  Trace <span className="font-mono text-[0.85rem]">{selectedTrace.slice(0, 16)}</span>
                </h2>
                <span className="text-[0.8rem] text-muted-foreground">
                  {selectedSpans.length} spans &middot; {fmtNs(total)}
                </span>
              </div>
              <div className="mb-3 flex flex-wrap gap-3">
                {Array.from(colors.entries()).map(([svc, color]) => (
                  <span key={svc} className="inline-flex items-center gap-1.5 text-[0.7rem] text-muted-foreground">
                    <span className="h-2 w-2 rounded-full" style={{ background: color }} />
                    {svc}
                  </span>
                ))}
              </div>

              <div className="rounded-md border border-border bg-card">
                {/* Tick ruler over the timeline column */}
                <div className="flex border-b border-border text-[0.65rem] text-muted-foreground">
                  <div className="w-[280px] shrink-0 border-r border-border px-3 py-1 font-medium uppercase tracking-wide">
                    Span
                  </div>
                  <div className="relative h-6 flex-1">
                    {[0, 0.25, 0.5, 0.75, 1].map((f) => (
                      <span
                        key={f}
                        className="absolute top-1"
                        style={f === 1 ? { right: 4 } : { left: `calc(${f * 100}% + 4px)` }}
                      >
                        {fmtNs(total * f)}
                      </span>
                    ))}
                  </div>
                </div>

                {rows.map(({ span, depth }) => {
                  const dur = span.end_time_unix_nano - span.start_time_unix_nano;
                  const left = ((span.start_time_unix_nano - window.start) / total) * 100;
                  const width = Math.max((dur / total) * 100, 0.5);
                  const color = colors.get(span.service_name || "unknown") || SERVICE_COLORS[0];
                  const isOpen = expanded.has(span.span_id);
                  const labelInside = width > 12;
                  return (
                    <div key={span.span_id} className="border-b border-border/50 last:border-b-0">
                      <button
                        onClick={() => toggle(span.span_id)}
                        aria-expanded={isOpen}
                        className="flex w-full items-stretch text-left hover:bg-primary/5"
                      >
                        <div
                          className="flex w-[280px] shrink-0 items-center gap-1.5 truncate border-r border-border px-3 py-1.5 text-[0.75rem]"
                          style={{ paddingLeft: 12 + depth * 14 }}
                        >
                          <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: color }} />
                          <span className="truncate font-medium">{span.name}</span>
                        </div>
                        <div className="relative flex-1 self-stretch py-1.5">
                          {[0.25, 0.5, 0.75].map((f) => (
                            <span
                              key={f}
                              className="absolute inset-y-0 w-px bg-border/40"
                              style={{ left: `${f * 100}%` }}
                            />
                          ))}
                          <div className="relative mx-1 h-4">
                            <div
                              className="absolute h-4 rounded-[3px]"
                              style={{
                                left: `${left}%`,
                                width: `${width}%`,
                                background: color,
                                opacity: span.status_code === 2 ? 1 : 0.75,
                                outline: span.status_code === 2 ? "1px solid #e05d5d" : undefined,
                              }}
                            />
                            <span
                              className="absolute top-0 text-[0.65rem] leading-4 text-muted-foreground"
                              style={
                                labelInside
                                  ? { left: `calc(${left}% + 4px)`, color: "#fff" }
                                  : left + width > 85
                                    ? { right: `calc(${100 - left}% + 4px)` }
                                    : { left: `calc(${left + width}% + 4px)` }
                              }
                            >
                              {fmtNs(dur)}
                            </span>
                          </div>
                        </div>
                      </button>
                      {isOpen && span.attributes.length > 0 && (
                        <div className="flex flex-wrap gap-1 border-t border-border/50 bg-secondary/40 px-3 py-2">
                          {span.attributes.map(([k, v]) => (
                            <span
                              key={k}
                              className="rounded bg-secondary px-1.5 py-0.5 font-mono text-[0.65rem] text-muted-foreground"
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
