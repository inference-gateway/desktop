import { useState } from "react";
import { Trash2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";

function ChatItem({ index }: { index: number }) {
  const { conversations, selected, sessionId, onChatClick, deleteConversation, isRunning, isAwaitingApproval } =
    useDesktop();
  const conv = conversations[index];
  const [confirm, setConfirm] = useState(false);
  const isSelected = selected.has(conv.id);
  const isActive = conv.id === sessionId;
  const running = isRunning(conv.id);
  const awaiting = isAwaitingApproval(conv.id);

  return (
    <div
      role="listitem"
      title={conv.title || conv.id}
      aria-selected={isSelected || undefined}
      onClick={(e) => onChatClick(index, e)}
      className={cn(
        "group flex cursor-pointer items-center gap-1 rounded-md px-[0.6rem] py-2 text-[0.83rem] text-muted-foreground hover:bg-card hover:text-foreground",
        isActive && "bg-card font-medium text-foreground shadow-[inset_3px_0_0_var(--primary)]",
        isSelected && "bg-primary/10 text-foreground shadow-[inset_3px_0_0_var(--primary)] ring-1 ring-primary/25"
      )}
    >
      {running && (
        <span
          aria-label={awaiting ? "Awaiting approval" : "Running"}
          title={awaiting ? "Awaiting approval" : "Running"}
          className={cn(
            "h-2 w-2 shrink-0 rounded-full",
            awaiting ? "bg-amber-500" : "animate-pulse bg-emerald-500"
          )}
        />
      )}
      <span className="min-w-0 flex-1 truncate">{conv.title || "(untitled)"}</span>
      <button
        aria-label="Delete conversation"
        title={confirm ? "Click again to delete" : "Delete conversation"}
        onClick={(e) => {
          e.stopPropagation();
          if (!confirm) {
            setConfirm(true);
            return;
          }
          deleteConversation(conv.id);
        }}
        onMouseLeave={() => setConfirm(false)}
        className={cn(
          "inline-flex shrink-0 items-center justify-center rounded-md p-[0.15rem] opacity-0 group-hover:opacity-100 focus-visible:opacity-100",
          confirm ? "bg-destructive text-white opacity-100" : "text-muted-foreground hover:bg-card hover:text-destructive"
        )}
      >
        <Trash2 size={14} />
      </button>
    </div>
  );
}

function BulkDeleteBar({ count, onDelete }: { count: number; onDelete: () => void }) {
  const [armed, setArmed] = useState(false);
  return (
    <div className="shrink-0 border-t border-border py-2">
      <button
        onClick={() => {
          if (!armed) {
            setArmed(true);
            return;
          }
          onDelete();
        }}
        onMouseLeave={() => setArmed(false)}
        className="w-full rounded-md bg-destructive px-3 py-2 text-[0.83rem] font-medium text-white hover:bg-danger-hover"
      >
        {armed ? `Click again to delete ${count}` : `Delete ${count} selected`}
      </button>
    </div>
  );
}

export function ChatList() {
  const { conversations, selected, bulkDelete } = useDesktop();
  return (
    <>
      <div id="chat-list" role="list" className="flex flex-1 flex-col gap-[2px] overflow-y-auto">
        {conversations.map((c, i) => (
          <ChatItem key={c.id} index={i} />
        ))}
      </div>
      {selected.size > 0 && <BulkDeleteBar count={selected.size} onDelete={bulkDelete} />}
    </>
  );
}
