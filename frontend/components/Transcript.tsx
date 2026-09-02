import { useCallback, useEffect, useRef, useState } from "react";
import { Check, ChevronDown, Download, Loader2, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { CopyButton } from "@/components/CopyButton";
import { Button } from "@/components/ui/button";
import { useDesktop } from "@/store";
import { handleLinkClick, renderMarkdown } from "@/lib/markdown";
import { api } from "@/lib/tauri";
import { prettyJson } from "@/lib/tools";
import type { ScheduleJob } from "@/lib/tauri";
import { COMPUTER_USE_TOOLS, type TranscriptItem } from "@/lib/transcript";

const BUBBLE = "rounded-xl px-4 py-[0.7rem] leading-[1.5] break-words shadow-sm";

function UserBubble({ text }: { text: string }) {
  return (
    <div className="flex max-w-[min(72ch,82%)] flex-col items-end gap-1 self-end">
      <div className={cn(BUBBLE, "rounded-br-[4px] bg-user text-user-foreground")}>{text}</div>
      <CopyButton text={text} />
    </div>
  );
}

function AssistantBubble({ chunks }: { chunks: string[] }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelectorAll("img").forEach((img) => {
      img.onerror = () => {
        img.outerHTML = '<span class="image-error">Image failed to load</span>';
      };
    });
  }, [chunks]);
  if (chunks.length === 0) return null;
  // Chunks are streamed content deltas; join before rendering so markdown
  // parses as one document instead of fragmenting per token.
  const merged = chunks.join("");
  return (
    <div className="flex max-w-[min(72ch,82%)] flex-col gap-1 self-start">
      <div
        ref={ref}
        className={cn(BUBBLE, "md rounded-bl-[4px] bg-assistant text-assistant-foreground")}
        onClick={(e) => handleLinkClick(e, (url) => api.openUrl(url).catch(() => {}))}
        dangerouslySetInnerHTML={{ __html: renderMarkdown(merged) }}
      />
      <CopyButton text={merged} />
    </div>
  );
}

function ReasoningBlock({ paragraphs }: { paragraphs: string[] }) {
  return (
    <details className="disclosure max-w-[min(72ch,82%)] self-start overflow-hidden rounded-md border border-border bg-secondary text-[0.85rem] text-muted-foreground">
      <summary className="px-[0.65rem] py-[0.35rem] hover:text-foreground">Thought process</summary>
      <div className="border-t border-border">
        <p className="whitespace-pre-wrap px-[0.65rem] py-2 italic leading-[1.5]">{paragraphs.join("")}</p>
      </div>
    </details>
  );
}

function ToolCard({ item }: { item: Extract<TranscriptItem, { kind: "tool" }> }) {
  const failed = item.state === "failed";
  const pre = [prettyJson(item.args), item.output ?? ""].filter(Boolean).join("\n\n");
  return (
    <div className="flex max-w-[min(72ch,82%)] flex-col gap-1 self-start">
      <details
        className={cn(
          "disclosure overflow-hidden rounded-md border border-l-[3px] font-mono text-[0.82rem]",
          failed ? "border-err-border border-l-destructive bg-err-bg" : "border-tool-border border-l-tool bg-tool-bg",
        )}
      >
        <summary className="overflow-hidden text-ellipsis whitespace-nowrap px-[0.65rem] py-[0.35rem]">
          <span className={cn("font-bold", failed ? "text-err" : "text-tool")}>{item.name}</span>
          <span className="ml-[0.4rem] text-muted-foreground">{item.args}</span>
          {item.state === "running" && <span className="ml-[0.4rem] animate-pulse text-muted-foreground">…</span>}
        </summary>
        {item.skeleton && item.state === "running" && <div className="image-skeleton mx-[0.65rem]" />}
        <pre
          className={cn(
            "m-0 max-h-80 overflow-auto whitespace-pre-wrap break-words px-[0.65rem] py-[0.55rem] leading-[1.45] text-foreground",
            failed ? "bg-err-bg" : "bg-tool-bg",
          )}
        >
          {pre}
        </pre>
      </details>
      <CopyButton text={pre} />
    </div>
  );
}

function ApprovalCard({
  item,
  approve,
}: {
  item: Extract<TranscriptItem, { kind: "approval" }>;
  approve: (callId: string, approved: boolean, scope?: "always") => void;
}) {
  if (item.status !== "pending") {
    const label = item.status === "approved" ? "✓ Approved" : item.status === "denied" ? "✕ Denied" : "Session ended";
    const color =
      item.status === "approved" ? "text-tool" : item.status === "denied" ? "text-err" : "text-muted-foreground";
    return (
      <div className={cn("self-start text-[0.8rem]", color)}>
        {label} {item.toolName}
      </div>
    );
  }
  return (
    <div className="self-stretch rounded-xl border border-warn bg-warn-bg p-3 text-[0.85rem]">
      <div className="mb-2 font-bold text-warn">Tool requires approval</div>
      <div className="mb-1">
        <span className="font-bold text-tool">{item.toolName}</span>
      </div>
      <pre className="mb-2 mt-1 max-h-[200px] overflow-y-auto whitespace-pre-wrap break-words rounded-md border border-border bg-card p-2 font-mono text-[0.8rem]">
        {item.toolArgs}
      </pre>
      <div className="flex gap-2">
        <button
          onClick={() => approve(item.callId, true)}
          className="flex items-center gap-2 rounded-md bg-primary px-4 py-[0.4rem] text-[0.85rem] text-primary-foreground hover:bg-primary-hover"
        >
          Approve
          <kbd aria-hidden="true" className="rounded border border-white/35 px-1.5 font-mono text-[0.7rem]">
            A
          </kbd>
        </button>
        {item.toolName === "SandboxAccess" && (
          <button
            onClick={() => approve(item.callId, true, "always")}
            className="flex items-center gap-2 rounded-md bg-primary px-4 py-[0.4rem] text-[0.85rem] text-primary-foreground hover:bg-primary-hover"
          >
            Always allow
          </button>
        )}
        <button
          onClick={() => approve(item.callId, false)}
          className="flex items-center gap-2 rounded-md bg-destructive px-4 py-[0.4rem] text-[0.85rem] text-white hover:bg-danger-hover"
        >
          Deny
          <kbd aria-hidden="true" className="rounded border border-white/35 px-1.5 font-mono text-[0.7rem]">
            R
          </kbd>
        </button>
      </div>
    </div>
  );
}

function TypingBubble({ label }: { label: string | null }) {
  return (
    <div className={cn(BUBBLE, "flex items-center gap-[0.35rem] self-start rounded-bl-[4px] bg-assistant")}>
      <div className="typing-dots flex items-center gap-[0.35rem]">
        <span />
        <span />
        <span />
      </div>
      {label && <span className="ml-1 text-[0.78rem] text-muted-foreground">{label}</span>}
    </div>
  );
}

function ImageDownload({ filename, src, path }: { filename: string; src: string; path: string }) {
  const [status, setStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const save = () => {
    if (status === "saving") return;
    setStatus("saving");
    api
      .saveImage(path)
      .then(() => setStatus("saved"))
      .catch(() => setStatus("error"));
  };
  useEffect(() => {
    if (status !== "saved" && status !== "error") return;
    const t = setTimeout(() => setStatus("idle"), 2000);
    return () => clearTimeout(t);
  }, [status]);
  const Icon = status === "saving" ? Loader2 : status === "saved" ? Check : status === "error" ? X : Download;
  return (
    <div className="group relative my-2 inline-block max-w-full">
      <img className="block h-auto w-full rounded-md" data-infer={filename} src={src} alt="" />
      {path && (
        <Button
          type="button"
          size="icon-sm"
          variant="secondary"
          onClick={save}
          disabled={status === "saving"}
          aria-label="Download image"
          className={cn(
            "absolute right-2 top-2 opacity-0 shadow-sm backdrop-blur-sm transition-opacity focus-visible:opacity-100 group-hover:opacity-100 disabled:opacity-100",
            status === "saved" && "text-green-600 dark:text-green-500",
            status === "error" && "text-destructive",
          )}
        >
          <Icon className={cn(status === "saving" && "animate-spin")} />
        </Button>
      )}
    </div>
  );
}

function Item({
  item,
  approve,
}: {
  item: TranscriptItem;
  approve: (callId: string, approved: boolean, scope?: "always") => void;
}) {
  switch (item.kind) {
    case "user":
      return <UserBubble text={item.text} />;
    case "assistant":
      return <AssistantBubble chunks={item.chunks} />;
    case "reasoning":
      return <ReasoningBlock paragraphs={item.paragraphs} />;
    case "tool":
      return <ToolCard item={item} />;
    case "approval":
      return <ApprovalCard item={item} approve={approve} />;
    case "image":
      return <ImageDownload filename={item.filename} src={item.src} path={item.path} />;
    case "audio":
      return (
        <div className="flex max-w-[min(72ch,82%)] flex-col gap-1 self-start">
          <audio
            controls
            preload="none"
            src={item.src}
            aria-label={`Generated speech ${item.filename}`}
            className="w-full"
          />
        </div>
      );
    case "error":
      return (
        <div className="max-w-[min(72ch,82%)] self-start rounded-md border border-err-border bg-err-bg px-3 py-2 text-err">
          {item.text}
        </div>
      );
    case "cancelled":
      return (
        <div className="self-center rounded-md bg-secondary px-3 py-[0.35rem] text-[0.85rem] italic text-muted-foreground">
          Cancelled
        </div>
      );
  }
}

function ScheduledJobs() {
  const [jobs, setJobs] = useState<ScheduleJob[]>([]);
  const [githubRepo, setGithubRepo] = useState<string | null>(null);
  useEffect(() => {
    api
      .getConfig()
      .then((c) => setGithubRepo(c.scheduler_backend === "github" ? c.scheduler_github_repository : null))
      .catch(() => {});
  }, []);
  useEffect(() => {
    let cancelled = false;
    const refresh = () =>
      api
        .listSchedules()
        .then((j) => {
          if (!cancelled) setJobs(j);
        })
        .catch(() => {});
    refresh();
    const t = setInterval(refresh, 10000);
    return () => {
      cancelled = true;
      clearInterval(t);
    };
  }, []);
  if (jobs.length === 0) return null;
  return (
    <details
      id="scheduled-jobs"
      className="disclosure shrink-0 overflow-hidden border-b border-border bg-secondary text-[0.85rem] text-muted-foreground"
    >
      <summary className="px-5 py-[0.35rem] hover:text-foreground">
        Scheduled jobs ({jobs.length})
        {githubRepo !== null && (
          <span className="ml-2 text-[0.75rem]">
            runs on GitHub Actions (UTC)
            {githubRepo.includes("/") && (
              <>
                {" - "}
                <button
                  className="underline hover:text-foreground"
                  onClick={(e) => {
                    e.preventDefault();
                    api.openUrl(`https://github.com/${githubRepo}/actions`).catch(() => {});
                  }}
                >
                  view runs
                </button>
              </>
            )}
          </span>
        )}
      </summary>
      <div className="border-t border-border">
        {jobs.map((j) => (
          <div
            key={j.id}
            className="flex items-baseline gap-2 border-b border-border px-5 py-1.5 last:border-b-0"
            title={j.description || j.prompt}
          >
            <span className="shrink-0 whitespace-nowrap font-medium text-foreground">{j.name || j.id}</span>
            <code className="shrink-0 whitespace-nowrap rounded bg-card px-1 text-[0.75rem]">{j.cron_expression}</code>
            {j.run_once && <span className="shrink-0 text-[0.75rem]">one-off</span>}
            <span className="ml-auto min-w-0 truncate text-[0.75rem]">{j.prompt}</span>
            {j.last_error && (
              <span className="shrink-0 text-[0.75rem] text-err" title={j.last_error}>
                failed
              </span>
            )}
          </div>
        ))}
      </div>
    </details>
  );
}

const SCROLL_THRESHOLD = 2;

export function Transcript() {
  const { items, typing, approve, runLabel, sessionId } = useDesktop();
  const ref = useRef<HTMLDivElement>(null);
  const isAtBottomRef = useRef(true);
  const [showScrollButton, setShowScrollButton] = useState(false);
  const pendingApproval = [...items]
    .reverse()
    .find(
      (item): item is Extract<TranscriptItem, { kind: "approval" }> =>
        item.kind === "approval" && item.status === "pending" && !COMPUTER_USE_TOOLS.has(item.toolName),
    );

  useEffect(() => {
    if (!pendingApproval) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || event.altKey || event.ctrlKey || event.metaKey) {
        return;
      }
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable ||
          (target instanceof HTMLInputElement && !target.disabled && !target.readOnly) ||
          (target instanceof HTMLTextAreaElement && !target.disabled && !target.readOnly) ||
          (target instanceof HTMLSelectElement && !target.disabled))
      ) {
        return;
      }
      const key = event.key.toLowerCase();
      if (key !== "a" && key !== "r") return;
      event.preventDefault();
      approve(pendingApproval.callId, key === "a");
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [approve, pendingApproval]);

  const checkAtBottom = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < SCROLL_THRESHOLD;
    isAtBottomRef.current = atBottom;
    setShowScrollButton(!atBottom);
  }, []);

  useEffect(() => {
    const el = ref.current;
    if (el && isAtBottomRef.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [items, typing]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onWheel = (event: WheelEvent) => {
      if (event.deltaY < 0) {
        isAtBottomRef.current = false;
        setShowScrollButton(true);
      }
    };
    el.addEventListener("scroll", checkAtBottom, { passive: true });
    el.addEventListener("wheel", onWheel, { passive: true });
    checkAtBottom();
    return () => {
      el.removeEventListener("scroll", checkAtBottom);
      el.removeEventListener("wheel", onWheel);
    };
  }, [checkAtBottom]);

  const scrollToBottom = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    isAtBottomRef.current = true;
    setShowScrollButton(false);
  }, []);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <ScheduledJobs />
      <div
        id="chat-transcript"
        role="log"
        aria-label="Chat transcript"
        ref={ref}
        className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-5 [&>*]:shrink-0"
      >
        {items.length === 0 && !typing && (
          <div className="m-auto text-[0.95rem] text-muted-foreground">Start a conversation</div>
        )}
        {items.map((item) => (
          <Item key={item.id} item={item} approve={approve} />
        ))}
        {typing && <TypingBubble label={sessionId ? (runLabel(sessionId)?.label ?? null) : null} />}
      </div>
      {showScrollButton && (
        <button
          onClick={scrollToBottom}
          aria-label="Scroll to latest"
          className="absolute bottom-4 right-4 z-10 flex size-9 items-center justify-center rounded-full border border-border bg-background shadow-md transition-colors hover:bg-secondary"
        >
          <ChevronDown className="size-5" />
        </button>
      )}
    </div>
  );
}
