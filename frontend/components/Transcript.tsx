import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Check, ChevronDown, Download, Loader2, RotateCcw, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { AudioPlayer } from "@/components/AudioPlayer";
import { CopyButton } from "@/components/CopyButton";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { useChatStream, useDesktop } from "@/store";
import { useSidebar } from "@/stores/sidebar";
import { useNow } from "@/hooks/useNow";
import { formatElapsed } from "@/lib/elapsed";
import { handleLinkClick, renderMarkdown } from "@/lib/markdown";
import { matchApprovalShortcut } from "@/lib/shortcuts";
import { api } from "@/lib/tauri";
import { isBashCommand, prettyJson } from "@/lib/tools";
import type { ScheduleJob, UserQuestionAnswer } from "@/lib/tauri";
import type { PlanAction } from "@/lib/agent-mode";
import {
  COMPUTER_USE_TOOLS,
  SANDBOX_ACCESS_TOOL,
  backgroundNoteHeader,
  followsBottom,
  visibleItems,
  type ToolState,
  type TranscriptItem,
} from "@/lib/transcript";

const BUBBLE = "rounded-xl px-4 py-[0.7rem] leading-[1.5] break-words shadow-sm";

/** Fetches the WAV through the chat's daemon, since a remote machine's file is not on this disk. */
function SpeechPlayer({ item }: { item: Extract<TranscriptItem, { kind: "audio" }> }) {
  const { sessionId } = useDesktop();
  const { projects } = useSidebar();
  const project = sessionId ? projects[sessionId] : undefined;
  const [src, setSrc] = useState<string | null>(null);

  useEffect(() => {
    if (!sessionId) {
      setSrc(item.src);
      return;
    }
    let alive = true;
    let url: string | null = null;
    api
      .readArtifact(sessionId, project, item.path)
      .then((bytes) => {
        url = URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
        if (alive) setSrc(url);
      })
      .catch(() => {
        if (alive) setSrc(item.src);
      });
    return () => {
      alive = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [sessionId, project, item.path, item.src]);

  if (!src) return null;
  const save = sessionId ? () => api.saveArtifact(sessionId, project, item.path) : undefined;
  return (
    <div className="flex max-w-[min(72ch,82%)] flex-col gap-1 self-start">
      <AudioPlayer src={src} ariaLabel={`generated speech ${item.filename}`} path={item.path} save={save} />
    </div>
  );
}

function UserBubble({ id, text }: { id: string; text: string }) {
  const { running, resendPrompt } = useDesktop();
  return (
    <div className="flex max-w-[min(72ch,82%)] flex-col items-end gap-1 self-end">
      <div className={cn(BUBBLE, "rounded-br-[4px] bg-user text-user-foreground")}>{text}</div>
      <div className="flex gap-1">
        <Button
          type="button"
          size="icon-sm"
          variant="secondary"
          disabled={running}
          aria-label="Resend prompt"
          title="Resend this prompt - everything after it is dropped"
          onClick={() => void resendPrompt(id)}
        >
          <RotateCcw />
        </Button>
        <CopyButton text={text} />
      </div>
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

function TaskResultCard({ item }: { item: Extract<TranscriptItem, { kind: "task_result" }> }) {
  const header = backgroundNoteHeader(item.text);
  const failed = header?.failed ?? false;
  const body = header
    ? item.text
        .trimStart()
        .slice(item.text.trimStart().indexOf("]") + 1)
        .trim()
    : item.text;
  return (
    <div className="flex max-w-[min(72ch,82%)] flex-col gap-1 self-start" data-testid="task-result">
      <details
        className={cn(
          "disclosure overflow-hidden rounded-md border border-l-[3px] font-mono text-[0.82rem]",
          failed ? "border-err-border border-l-destructive bg-err-bg" : "border-tool-border border-l-tool bg-tool-bg",
        )}
      >
        <summary className="overflow-hidden text-ellipsis whitespace-nowrap px-[0.65rem] py-[0.35rem]">
          {failed ? (
            <X className="mr-[0.4rem] inline h-3.5 w-3.5 text-err" aria-hidden />
          ) : (
            <Check className="mr-[0.4rem] inline h-3.5 w-3.5 text-tool" aria-hidden />
          )}
          <span className={cn("font-bold", failed ? "text-err" : "text-tool")}>
            {header ? `${header.kind} ${failed ? "failed" : "completed"}` : "Background task"}
          </span>
          {header && <span className="ml-[0.4rem] text-muted-foreground">{header.label}</span>}
        </summary>
        <pre
          className={cn(
            "m-0 max-h-80 overflow-auto whitespace-pre-wrap break-words px-[0.65rem] py-[0.55rem] leading-[1.45] text-foreground",
            failed ? "bg-err-bg" : "bg-tool-bg",
          )}
        >
          {body}
        </pre>
      </details>
      <CopyButton text={item.text} />
    </div>
  );
}

/** Title of the session that holds the agent recording a failed RecordStart
 * ran into, when that session is another chat in this app. */
function useRecordingOwnerTitle(item: Extract<TranscriptItem, { kind: "tool" }>): string | null {
  const { recordingOwner, sessionId, conversations } = useDesktop();
  if (item.name !== "RecordStart" || item.state !== "failed" || !recordingOwner || recordingOwner === sessionId) {
    return null;
  }
  return conversations.find((c) => c.id === recordingOwner)?.title || recordingOwner.slice(0, 5);
}

const TOOL_OK = { card: "border-tool-border border-l-tool bg-tool-bg", name: "text-tool", body: "bg-tool-bg" };

const TOOL_TONE: Record<ToolState, typeof TOOL_OK> = {
  running: TOOL_OK,
  done: TOOL_OK,
  failed: { card: "border-err-border border-l-destructive bg-err-bg", name: "text-err", body: "bg-err-bg" },
  cancelled: {
    card: "border-border border-l-muted-foreground bg-secondary",
    name: "text-muted-foreground",
    body: "bg-secondary",
  },
};

function ToolCard({ item }: { item: Extract<TranscriptItem, { kind: "tool" }> }) {
  const tone = TOOL_TONE[item.state];
  const recordingOwnerTitle = useRecordingOwnerTitle(item);
  const pre = [prettyJson(item.args), item.output ?? ""].filter(Boolean).join("\n\n");
  return (
    <div className="flex max-w-[min(72ch,82%)] flex-col gap-1 self-start">
      <details
        className={cn(
          "disclosure overflow-hidden rounded-md border border-l-[3px] font-mono text-[0.82rem]",
          tone.card,
        )}
      >
        <summary className="overflow-hidden text-ellipsis whitespace-nowrap px-[0.65rem] py-[0.35rem]">
          <span className={cn("font-bold", tone.name)}>{item.name}</span>
          {item.state === "cancelled" && <span className="ml-[0.4rem] italic text-muted-foreground">(cancelled)</span>}
          <span className="ml-[0.4rem] text-muted-foreground">{item.args}</span>
          {item.state === "running" && <span className="ml-[0.4rem] animate-pulse text-muted-foreground">…</span>}
        </summary>
        {item.skeleton && item.state === "running" && <div className="image-skeleton mx-[0.65rem]" />}
        <pre
          className={cn(
            "m-0 max-h-80 overflow-auto whitespace-pre-wrap break-words px-[0.65rem] py-[0.55rem] leading-[1.45] text-foreground",
            tone.body,
          )}
        >
          {pre}
        </pre>
      </details>
      {recordingOwnerTitle && (
        <span className="text-[0.78rem] text-err">Recording in progress in &ldquo;{recordingOwnerTitle}&rdquo;</span>
      )}
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
        {item.toolName === SANDBOX_ACCESS_TOOL && (
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
            D
          </kbd>
        </button>
      </div>
    </div>
  );
}

const PLAN_OUTCOMES: Record<PlanAction | "expired", string> = {
  accept: "✓ Plan accepted - running in auto mode",
  accept_standard: "✓ Plan accepted - approving each step",
  reject: "✕ Plan rejected",
  expired: "Session ended",
};

// The CLI's plan choices with its labels: Accept runs the plan in auto mode,
// Approve each step keeps per-action approval, Reject keeps planning.
function PlanCard({
  item,
  decide,
}: {
  item: Extract<TranscriptItem, { kind: "plan" }>;
  decide: (planId: string, action: PlanAction) => void;
}) {
  const plan = (
    <div
      className="md max-h-[320px] overflow-y-auto rounded-md border border-border bg-card p-2"
      onClick={(e) => handleLinkClick(e, (url) => api.openUrl(url).catch(() => {}))}
      dangerouslySetInnerHTML={{ __html: renderMarkdown(item.plan) }}
    />
  );
  if (item.status !== "pending") {
    return (
      <details className="self-stretch text-[0.8rem]">
        <summary
          className={cn(
            "cursor-pointer",
            item.status === "reject" || item.status === "expired" ? "text-muted-foreground" : "text-tool",
          )}
        >
          {PLAN_OUTCOMES[item.status]}
        </summary>
        <div className="mt-1 text-[0.85rem]">{plan}</div>
      </details>
    );
  }
  return (
    <div className="self-stretch rounded-xl border border-warn bg-warn-bg p-3 text-[0.85rem]">
      <div className="mb-2 font-bold text-warn">Review the plan</div>
      <div className="mb-2">{plan}</div>
      <div className="flex flex-wrap gap-2">
        <button
          onClick={() => decide(item.planId, "accept")}
          className="rounded-md bg-primary px-4 py-[0.4rem] text-[0.85rem] text-primary-foreground hover:bg-primary-hover"
        >
          Accept
        </button>
        <button
          onClick={() => decide(item.planId, "accept_standard")}
          className="rounded-md border border-border bg-card px-4 py-[0.4rem] text-[0.85rem] hover:bg-secondary"
        >
          Approve each step
        </button>
        <button
          onClick={() => decide(item.planId, "reject")}
          className="rounded-md bg-destructive px-4 py-[0.4rem] text-[0.85rem] text-white hover:bg-danger-hover"
        >
          Reject
        </button>
      </div>
    </div>
  );
}

const OTHER = "Other";
const RECOMMENDED = /\(Recommended\)\s*$/;

function answerSummary(a: UserQuestionAnswer, options: { label: string; description: string }[]): string {
  const withDescription = (label: string) => {
    const d = options.find((o) => o.label === label)?.description;
    return d ? `${label} (${d})` : label;
  };
  const parts = [a.selectedLabels.map(withDescription).join(", "), a.otherText ? `Other: "${a.otherText}"` : ""];
  return `[${a.header}] ${a.question} -> ${parts.filter(Boolean).join("; ") || "(no selection)"}`;
}

// Native radio/checkbox/text inputs so the e2e harness can drive the form
// through the macOS accessibility tree. The always-appended "Other" row selects
// itself when typed into, matching the CLI's own form.
function QuestionCard({
  item,
  answer,
}: {
  item: Extract<TranscriptItem, { kind: "question" }>;
  answer: (callId: string, answers: UserQuestionAnswer[] | null) => void;
}) {
  const [answers, setAnswers] = useState<UserQuestionAnswer[]>(() =>
    item.questions.map((q) => ({
      header: q.header,
      question: q.question,
      selectedLabels: q.options.filter((o) => RECOMMENDED.test(o.label)).map((o) => o.label),
      otherText: "",
    })),
  );
  if (item.status !== "pending") {
    const label = item.status === "answered" ? "✓ Answered" : item.status === "skipped" ? "Skipped" : "Session ended";
    const color = item.status === "answered" ? "text-tool" : "text-muted-foreground";
    return (
      <div className={cn("self-start text-[0.8rem]", color)}>
        {label}
        {item.answers.map((a, i) => (
          <div key={a.header + a.question} className="whitespace-pre-wrap text-foreground">
            {answerSummary(a, item.questions[i]?.options ?? [])}
          </div>
        ))}
      </div>
    );
  }
  const update = (i: number, patch: Partial<UserQuestionAnswer>) =>
    setAnswers((prev) => prev.map((a, j) => (j === i ? { ...a, ...patch } : a)));
  const toggle = (i: number, label: string, multi: boolean, checked: boolean) => {
    const current = answers[i].selectedLabels;
    const next = multi ? (checked ? [...current, label] : current.filter((l) => l !== label)) : [label];
    update(i, { selectedLabels: next, otherText: multi || label === OTHER ? answers[i].otherText : "" });
  };
  return (
    <div className="self-stretch rounded-xl border border-warn bg-warn-bg p-3 text-[0.85rem]">
      <div className="mb-2 font-bold text-warn">The agent has a question</div>
      {item.questions.map((q, i) => {
        const a = answers[i];
        const otherChecked = a.selectedLabels.includes(OTHER);
        return (
          <fieldset key={q.header + q.question} className="mb-3 border-0 p-0">
            <legend className="mb-1">
              <span className="mr-2 rounded bg-secondary px-1.5 py-0.5 font-mono text-[0.7rem]">{q.header}</span>
              <span className="font-bold">{q.question}</span>
            </legend>
            {q.options.map((o) => (
              <label key={o.label} className="flex cursor-pointer items-start gap-2 py-0.5">
                <input
                  type={q.multiSelect ? "checkbox" : "radio"}
                  name={`${item.callId}-${i}`}
                  aria-label={o.label}
                  className="mt-1"
                  checked={a.selectedLabels.includes(o.label)}
                  onChange={(e) => toggle(i, o.label, q.multiSelect, e.target.checked)}
                />
                <span>
                  {o.label}
                  {o.description && <span className="ml-2 text-muted-foreground">{o.description}</span>}
                </span>
              </label>
            ))}
            <label className="flex cursor-pointer items-center gap-2 py-0.5">
              <input
                type={q.multiSelect ? "checkbox" : "radio"}
                name={`${item.callId}-${i}`}
                aria-label={OTHER}
                checked={otherChecked}
                onChange={(e) => toggle(i, OTHER, q.multiSelect, e.target.checked)}
              />
              <span>{OTHER}</span>
              <input
                type="text"
                aria-label={`Other answer for ${q.header}`}
                placeholder="Type your own answer"
                value={a.otherText ?? ""}
                onChange={(e) => {
                  update(i, { otherText: e.target.value });
                  if (!otherChecked) toggle(i, OTHER, q.multiSelect, true);
                }}
                className="flex-1 rounded-md border border-border bg-card px-2 py-1 text-[0.8rem]"
              />
            </label>
          </fieldset>
        );
      })}
      <div className="flex gap-2">
        <button
          onClick={() =>
            answer(
              item.callId,
              answers.map((a) => ({ ...a, selectedLabels: a.selectedLabels.filter((l) => l !== OTHER) })),
            )
          }
          className="rounded-md bg-primary px-4 py-[0.4rem] text-[0.85rem] text-primary-foreground hover:bg-primary-hover"
        >
          Submit
        </button>
        <button
          onClick={() => answer(item.callId, null)}
          className="rounded-md bg-secondary px-4 py-[0.4rem] text-[0.85rem] hover:bg-accent"
        >
          Skip
        </button>
      </div>
    </div>
  );
}

function TypingBubble({ label, startedAt }: { label: string | null; startedAt: number | null }) {
  const now = useNow(startedAt !== null);
  return (
    <div className={cn(BUBBLE, "flex items-center gap-[0.35rem] self-start rounded-bl-[4px] bg-assistant")}>
      <div className="typing-dots flex items-center gap-[0.35rem]">
        <span />
        <span />
        <span />
      </div>
      {label && <span className="ml-1 text-[0.78rem] text-muted-foreground">{label}</span>}
      {startedAt !== null && (
        <span className="text-[0.78rem] tabular-nums text-muted-foreground/70">{formatElapsed(now - startedAt)}</span>
      )}
    </div>
  );
}

function ImageDownload({ filename, src, path }: { filename: string; src: string; path: string }) {
  const [status, setStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [enlarged, setEnlarged] = useState(false);
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
    <div className="relative my-2 inline-block max-w-full">
      <button
        type="button"
        aria-label="View image"
        title="View full size"
        className="block cursor-zoom-in rounded-md"
        onClick={() => setEnlarged(true)}
      >
        <img className="block h-auto max-h-40 w-auto max-w-60 rounded-md" data-infer={filename} src={src} alt="" />
      </button>
      <Dialog open={enlarged} onOpenChange={setEnlarged}>
        <DialogContent className="w-auto p-2 sm:max-w-[92vw]">
          <DialogTitle className="sr-only">{filename}</DialogTitle>
          <img className="block max-h-[88vh] max-w-full rounded-md" src={src} alt={filename} />
        </DialogContent>
      </Dialog>
      {path && (
        <Button
          type="button"
          size="icon-sm"
          variant="secondary"
          onClick={save}
          disabled={status === "saving"}
          aria-label="Download image"
          className={cn(
            "absolute right-2 top-2 shadow-sm backdrop-blur-sm",
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
  answer,
  decide,
}: {
  item: TranscriptItem;
  approve: (callId: string, approved: boolean, scope?: "always") => void;
  answer: (callId: string, answers: UserQuestionAnswer[] | null) => void;
  decide: (planId: string, action: PlanAction) => void;
}) {
  switch (item.kind) {
    case "user":
      return isBashCommand(item.text) ? null : <UserBubble id={item.id} text={item.text} />;
    case "assistant":
      return <AssistantBubble chunks={item.chunks} />;
    case "reasoning":
      return <ReasoningBlock paragraphs={item.paragraphs} />;
    case "tool":
      return <ToolCard item={item} />;
    case "task_result":
      return <TaskResultCard item={item} />;
    case "approval":
      return <ApprovalCard item={item} approve={approve} />;
    case "question":
      return <QuestionCard item={item} answer={answer} />;
    case "plan":
      return <PlanCard item={item} decide={decide} />;
    case "image":
      return <ImageDownload filename={item.filename} src={item.src} path={item.path} />;
    case "audio":
      return <SpeechPlayer item={item} />;
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
    case "info":
      return <div className="self-center text-[0.8rem] text-muted-foreground">{item.text}</div>;
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
  const { items, runLabel } = useChatStream();
  const { typing, approve, answerQuestions, decidePlan, sessionId, composerRef, turnStartedAt } = useDesktop();
  const ref = useRef<HTMLDivElement>(null);
  const isAtBottomRef = useRef(true);
  const lastScrollTopRef = useRef(0);
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
      const target = event.target;
      const editable =
        target instanceof HTMLElement &&
        (target.isContentEditable ||
          (target instanceof HTMLInputElement && !target.disabled && !target.readOnly) ||
          (target instanceof HTMLTextAreaElement && !target.disabled && !target.readOnly) ||
          (target instanceof HTMLSelectElement && !target.disabled));
      const action = matchApprovalShortcut({
        key: event.key,
        metaKey: event.metaKey,
        ctrlKey: event.ctrlKey,
        shiftKey: event.shiftKey,
        altKey: event.altKey,
        repeat: event.repeat,
        defaultPrevented: event.defaultPrevented,
        inComposer: target === composerRef.current,
        editable,
      });
      if (!action) return;
      event.preventDefault();
      approve(pendingApproval.callId, action === "approve");
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [approve, pendingApproval, composerRef]);

  const checkAtBottom = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < SCROLL_THRESHOLD;
    const following = followsBottom(isAtBottomRef.current, el.scrollTop < lastScrollTopRef.current, atBottom);
    lastScrollTopRef.current = el.scrollTop;
    isAtBottomRef.current = following;
    setShowScrollButton(!following);
  }, []);

  // Layout effect, not passive: pin synchronously per commit so scrollTop
  // tracks scrollHeight before any scroll event can observe a grown transcript
  // against a stale scrollTop. As a passive effect, rapid streaming let that
  // stale read flip isAtBottom to false, which stuck auto-follow off and left
  // a late approval card below the fold (transcript-overflow.yaml).
  useLayoutEffect(() => {
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
    const onMediaLoad = () => {
      if (isAtBottomRef.current) el.scrollTop = el.scrollHeight;
    };
    el.addEventListener("scroll", checkAtBottom, { passive: true });
    el.addEventListener("wheel", onWheel, { passive: true });
    el.addEventListener("load", onMediaLoad, true);
    checkAtBottom();
    return () => {
      el.removeEventListener("scroll", checkAtBottom);
      el.removeEventListener("wheel", onWheel);
      el.removeEventListener("load", onMediaLoad, true);
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
        {visibleItems(items).map((item) => (
          <Item key={item.id} item={item} approve={approve} answer={answerQuestions} decide={decidePlan} />
        ))}
        {typing && (
          <TypingBubble label={sessionId ? (runLabel(sessionId)?.label ?? null) : null} startedAt={turnStartedAt} />
        )}
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
