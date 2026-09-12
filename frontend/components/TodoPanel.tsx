// Pinned, editable todo list for the active session (#196). The list itself is
// derived from the agent's last TodoWrite call (see todosFrom); user edits live
// in a per-session draft until handed back to the agent with "Send to agent".
import { useEffect, useRef, useState } from "react";
import {
  ArrowDown,
  ArrowUp,
  Check,
  ChevronDown,
  Circle,
  CircleDot,
  GripVertical,
  Plus,
  Send,
  Trash2,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { todosDiffer, todosFrom, type TodoItem, type TodoStatus } from "@/lib/transcript";
import { useDesktop } from "@/store";

const STATUS_ORDER: TodoStatus[] = ["pending", "in_progress", "completed"];
const STATUS_ICON = { pending: Circle, in_progress: CircleDot, completed: Check } as const;
const STATUS_CLASS = { pending: "text-muted-foreground", in_progress: "text-warn", completed: "text-tool" } as const;

const ROW_BUTTON =
  "shrink-0 rounded p-0.5 text-muted-foreground hover:bg-secondary hover:text-foreground disabled:cursor-default disabled:opacity-30";

export function TodoPanel() {
  const { items, sessionId, running, sendText, todoDrafts, setTodoDraft } = useDesktop();
  const draftKey = sessionId ?? "new";
  const draft = todoDrafts[draftKey] ?? null;
  const agentTodos = todosFrom(items);
  const shown = draft ?? agentTodos;
  const dirty = draft !== null && todosDiffer(draft, agentTodos);
  const [open, setOpen] = useState(true);
  const [input, setInput] = useState("");
  const dragFrom = useRef<number | null>(null);

  // The agent's TodoWrite is the source of truth: once it catches up with a
  // hand-off, drop the draft so the panel is not stuck dirty forever.
  useEffect(() => {
    if (draft?.length && !todosDiffer(draft, agentTodos)) setTodoDraft(draftKey, null);
  }, [draft, agentTodos, draftKey, setTodoDraft]);

  // Hidden until the agent writes a list or the user starts one.
  if (draft === null && agentTodos.length === 0) {
    return (
      <div className="flex shrink-0 justify-center pb-1">
        <button
          onClick={() => setTodoDraft(draftKey, [])}
          aria-label="Add todo"
          className="flex items-center gap-1 rounded-md px-2 py-1 text-[0.78rem] text-muted-foreground hover:bg-secondary hover:text-foreground"
        >
          <Plus size={12} /> Todo
        </button>
      </div>
    );
  }

  const update = (next: TodoItem[]) => setTodoDraft(draftKey, next);

  const move = (from: number, to: number) => {
    if (from === to || to < 0 || to >= shown.length) return;
    const next = [...shown];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    update(next);
  };

  const add = () => {
    const content = input.trim();
    if (!content) return;
    setInput("");
    update([...shown, { content, status: "pending" }]);
  };

  const sendTodos = async () => {
    const list = shown.filter((t) => t.content.trim());
    if (list.length === 0) return;
    const ok = await sendText(
      `Update the session todo list to exactly the following by calling the TodoWrite tool now with this argument:\n${JSON.stringify({ todos: list })}`,
    );
    if (ok && sessionId === null) setTodoDraft("new", null);
  };

  return (
    <div className="mx-5 mb-1 shrink-0 overflow-hidden rounded-md border border-tool-border bg-tool-bg text-[0.85rem]">
      <div className="flex items-center gap-2 px-3 py-[0.35rem]">
        <button
          onClick={() => setOpen(!open)}
          aria-expanded={open}
          aria-label="Toggle todo panel"
          className="flex items-center gap-1.5 font-bold text-tool hover:text-foreground"
        >
          <ChevronDown size={14} className={cn("transition-transform", !open && "-rotate-90")} />
          Todos ({shown.length})
        </button>
        {dirty && (
          <button
            onClick={sendTodos}
            disabled={running}
            title={running ? "Stop the current run first" : "Send this list to the agent"}
            className="ml-auto flex items-center gap-1 rounded-md bg-primary px-2 py-[0.15rem] text-[0.78rem] text-primary-foreground hover:bg-primary-hover disabled:cursor-default disabled:opacity-50"
          >
            <Send size={12} /> Send to agent
          </button>
        )}
      </div>
      {open && (
        <div className="border-t border-border">
          <ul>
            {shown.map((t, i) => {
              const StatusIcon = STATUS_ICON[t.status];
              return (
                <li
                  key={i}
                  className="flex items-center gap-1 border-b border-border px-2 py-1 last:border-b-0"
                  onDragOver={(e) => e.preventDefault()}
                  onDrop={() => {
                    if (dragFrom.current !== null) move(dragFrom.current, i);
                    dragFrom.current = null;
                  }}
                >
                  <span
                    draggable
                    aria-hidden="true"
                    onDragStart={(e) => {
                      dragFrom.current = i;
                      e.dataTransfer.effectAllowed = "move";
                      e.dataTransfer.setData("text/plain", String(i));
                    }}
                    className="cursor-grab text-muted-foreground"
                  >
                    <GripVertical size={12} />
                  </span>
                  <button
                    onClick={() =>
                      update(
                        shown.map((it, j) =>
                          j === i
                            ? {
                                ...it,
                                status: STATUS_ORDER[(STATUS_ORDER.indexOf(t.status) + 1) % STATUS_ORDER.length],
                              }
                            : it,
                        ),
                      )
                    }
                    aria-label={`Todo status: ${t.status}`}
                    title={`${t.status} - click to advance`}
                    className={cn(ROW_BUTTON, STATUS_CLASS[t.status])}
                  >
                    <StatusIcon size={13} />
                  </button>
                  <input
                    value={t.content}
                    onChange={(e) => update(shown.map((it, j) => (j === i ? { ...it, content: e.target.value } : it)))}
                    aria-label="Todo content"
                    className={cn(
                      "min-w-0 flex-1 bg-transparent py-0.5 outline-none",
                      t.status === "completed" && "text-muted-foreground line-through",
                    )}
                  />
                  <button
                    onClick={() => move(i, i - 1)}
                    disabled={i === 0}
                    aria-label="Move todo up"
                    className={ROW_BUTTON}
                  >
                    <ArrowUp size={13} />
                  </button>
                  <button
                    onClick={() => move(i, i + 1)}
                    disabled={i === shown.length - 1}
                    aria-label="Move todo down"
                    className={ROW_BUTTON}
                  >
                    <ArrowDown size={13} />
                  </button>
                  <button
                    onClick={() => update(shown.filter((_, j) => j !== i))}
                    aria-label="Delete todo"
                    className={ROW_BUTTON}
                  >
                    <Trash2 size={13} />
                  </button>
                </li>
              );
            })}
          </ul>
          <div className="flex items-center gap-1 border-t border-border px-2 py-1">
            <Plus size={13} className="shrink-0 text-muted-foreground" />
            <input
              value={input}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") add();
              }}
              placeholder="Add a todo and press Enter"
              aria-label="Add a todo"
              className="min-w-0 flex-1 bg-transparent outline-none placeholder:text-muted-foreground"
            />
          </div>
        </div>
      )}
    </div>
  );
}
