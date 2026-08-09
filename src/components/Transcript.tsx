import { useEffect, useRef } from "react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { renderMarkdown } from "@/lib/markdown";
import { prettyJson } from "@/lib/tools";
import type { TranscriptItem } from "@/lib/transcript";

const BUBBLE = "max-w-[min(72ch,82%)] rounded-xl px-4 py-[0.7rem] leading-[1.5] break-words shadow-sm";

function UserBubble({ text }: { text: string }) {
  return <div className={cn(BUBBLE, "self-end rounded-br-[4px] bg-user text-user-foreground")}>{text}</div>;
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
  return (
    <div ref={ref} className={cn(BUBBLE, "md self-start rounded-bl-[4px] bg-assistant text-assistant-foreground")}>
      {chunks.map((c, i) => (
        <div key={i} dangerouslySetInnerHTML={{ __html: renderMarkdown(c) }} />
      ))}
    </div>
  );
}

function ReasoningBlock({ paragraphs }: { paragraphs: string[] }) {
  return (
    <details className="disclosure max-w-[min(72ch,82%)] self-start overflow-hidden rounded-md border border-border bg-secondary text-[0.85rem] text-muted-foreground">
      <summary className="px-[0.65rem] py-[0.35rem] hover:text-foreground">Thought process</summary>
      <div className="border-t border-border">
        {paragraphs.map((p, i) => (
          <p key={i} className="whitespace-pre-wrap px-[0.65rem] py-2 italic leading-[1.5]">
            {p}
          </p>
        ))}
      </div>
    </details>
  );
}

function ToolCard({ item }: { item: Extract<TranscriptItem, { kind: "tool" }> }) {
  const failed = item.state === "failed";
  const pre = [prettyJson(item.args), item.output ?? ""].filter(Boolean).join("\n\n");
  return (
    <details
      className={cn(
        "disclosure max-w-[min(72ch,82%)] self-start overflow-hidden rounded-md border border-l-[3px] font-mono text-[0.82rem]",
        failed ? "border-err-border border-l-destructive bg-err-bg" : "border-tool-border border-l-tool bg-tool-bg"
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
          failed ? "bg-err-bg" : "bg-tool-bg"
        )}
      >
        {pre}
      </pre>
    </details>
  );
}

function ApprovalCard({
  item,
  approve,
}: {
  item: Extract<TranscriptItem, { kind: "approval" }>;
  approve: (callId: string, approved: boolean) => void;
}) {
  const decided = item.status !== "pending";
  return (
    <div
      className={cn(
        "self-stretch rounded-xl border p-3 text-[0.85rem]",
        item.status === "approved"
          ? "border-tool bg-tool-bg"
          : item.status === "denied"
            ? "border-err-border bg-err-bg"
            : "border-warn bg-warn-bg"
      )}
    >
      <div className="mb-2 font-bold text-warn">Tool requires approval</div>
      <div className="mb-1">
        <span className="font-bold text-tool">{item.toolName}</span>
      </div>
      <pre className="mb-2 mt-1 max-h-[200px] overflow-y-auto whitespace-pre-wrap break-words rounded-md border border-border bg-card p-2 font-mono text-[0.8rem]">
        {item.toolArgs}
      </pre>
      {decided ? (
        <div className={cn("mt-2 text-[0.85rem] font-bold", item.status === "approved" ? "text-tool" : "text-err")}>
          {item.status === "approved" ? "Approved" : "Denied"}
        </div>
      ) : (
        <div className="flex gap-2">
          <button
            onClick={() => approve(item.callId, true)}
            className="rounded-md bg-primary px-4 py-[0.4rem] text-[0.85rem] text-primary-foreground hover:bg-primary-hover"
          >
            Approve
          </button>
          <button
            onClick={() => approve(item.callId, false)}
            className="rounded-md bg-destructive px-4 py-[0.4rem] text-[0.85rem] text-white hover:bg-danger-hover"
          >
            Deny
          </button>
        </div>
      )}
    </div>
  );
}

function TypingBubble() {
  return (
    <div className={cn(BUBBLE, "typing-dots flex items-center gap-[0.35rem] self-start rounded-bl-[4px] bg-assistant")}>
      <span />
      <span />
      <span />
    </div>
  );
}

function Item({ item, approve }: { item: TranscriptItem; approve: (callId: string, approved: boolean) => void }) {
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
      return (
        <img
          className="generated-image my-2 block h-auto w-full max-w-[320px] rounded-md"
          data-infer={item.filename}
          src={item.src}
          alt=""
        />
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

export function Transcript() {
  const { items, typing, approve } = useDesktop();
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [items, typing]);

  return (
    <div
      id="chat-transcript"
      role="log"
      aria-label="Chat transcript"
      ref={ref}
      className="flex flex-1 flex-col gap-2 overflow-y-auto p-5 [&>*]:shrink-0"
    >
      {items.length === 0 && !typing && (
        <div className="m-auto text-[0.95rem] text-muted-foreground">Start a conversation</div>
      )}
      {items.map((item) => (
        <Item key={item.id} item={item} approve={approve} />
      ))}
      {typing && <TypingBubble />}
    </div>
  );
}
