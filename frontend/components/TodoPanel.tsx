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
import { formatElapsed } from "@/lib/elapsed";
import { currentTodo, todoElapsedMs, todosDiffer, todosFrom, type TodoItem, type TodoStatus } from "@/lib/transcript";
import { useNow } from "@/hooks/useNow";
import { usePointerDrag } from "@/hooks/usePointerDrag";
import { useChatStream, useDesktop } from "@/store";

const STATUS_ORDER: TodoStatus[] = ["pending", "in_progress", "completed"];
const STATUS_ICON = { pending: Circle, in_progress: CircleDot, completed: Check } as const;
const STATUS_CLASS = { pending: "text-muted-foreground", in_progress: "text-warn", completed: "text-tool" } as const;

const PEEK_MS = 5000;

const TODO_TARGET = "todo:";

/** The edge a dragged row lands on: move(from, to) puts it after the hovered row
 * when dragging down and before it when dragging up. */
function dropEdgeClass(from: number, to: number): string {
  return to > from ? "shadow-[inset_0_-2px_0_0_var(--tool-fg)]" : "shadow-[inset_0_2px_0_0_var(--tool-fg)]";
}
const ROW_BUTTON =
  "shrink-0 rounded p-0.5 text-muted-foreground hover:bg-secondary hover:text-foreground disabled:cursor-default disabled:opacity-30";
const ROW_EDIT = "opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100";

function Elapsed({ todo, now }: { todo: TodoItem; now: number }) {
  const elapsed = todoElapsedMs(todo, now);
  if (elapsed === null) return null;
  return (
    <span
      title={todo.status === "in_progress" ? "Time worked on so far" : "Time worked on"}
      className={cn(
        "shrink-0 text-[0.75rem] tabular-nums",
        todo.status === "in_progress" ? "text-warn" : "text-muted-foreground",
      )}
    >
      {formatElapsed(elapsed)}
    </span>
  );
}

/** Pinned, editable todo list for the active session. The list comes from the
 * agent's last TodoWrite call (see todosFrom), and user edits live in a
 * per-session draft until "Send to agent" hands them back. */
export function TodoPanel() {
  const { items } = useChatStream();
  const { sessionId, running, sendText, todoDrafts, setTodoDraft } = useDesktop();
  const draftKey = sessionId ?? "new";
  const draft = todoDrafts[draftKey] ?? null;
  const agentTodos = todosFrom(items);
  const shown = draft ?? agentTodos;
  const dirty = draft !== null && todosDiffer(draft, agentTodos);
  const [open, setOpen] = useState(false);
  const peek = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const written = agentTodos.length > 0;

  const writtenThisRun = written && running;
  useEffect(() => {
    if (!writtenThisRun) return;
    setOpen(true);
    peek.current = setTimeout(() => setOpen(false), PEEK_MS);
    return () => clearTimeout(peek.current);
  }, [written, draftKey]);

  const show = (next: boolean) => {
    clearTimeout(peek.current);
    setOpen(next);
  };
  const [input, setInput] = useState("");
  const dragFrom = useRef<number | null>(null);
  const current = currentTodo(shown);
  const done = shown.filter((t) => t.status === "completed").length;
  const now = useNow(shown.some((t) => t.status === "in_progress" && Boolean(t.started_at)));

  useEffect(() => {
    const agentCaughtUpWithDraft = draft?.length && !todosDiffer(draft, agentTodos);
    if (agentCaughtUpWithDraft) setTodoDraft(draftKey, null);
  }, [draft, agentTodos, draftKey, setTodoDraft]);

  const update = (next: TodoItem[]) => setTodoDraft(draftKey, next);

  const move = (from: number, to: number) => {
    if (from === to || to < 0 || to >= shown.length) return;
    const next = [...shown];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    update(next);
  };
  const drag = usePointerDrag<number>((from, target) => {
    if (target.startsWith(TODO_TARGET)) move(from, Number(target.slice(TODO_TARGET.length)));
  });

  const noListYet = draft === null && agentTodos.length === 0;
  if (noListYet) {
    return (
      <div className="flex shrink-0 justify-center pb-1">
        <button
          onClick={() => {
            show(true);
            setTodoDraft(draftKey, []);
          }}
          aria-label="Add todo"
          className="flex items-center gap-1 rounded-md px-2 py-1 text-[0.78rem] text-muted-foreground hover:bg-secondary hover:text-foreground"
        >
          <Plus size={12} /> Todo
        </button>
      </div>
    );
  }

  const add = () => {
    const content = input.trim();
    if (!content) return;
    setInput("");
    update([...shown, { content, status: "pending" }]);
  };

  const sendTodos = async () => {
    const list = shown
      .filter((t) => t.content.trim())
      .map(({ id, content, status }) => ({ ...(id && { id }), content, status }));
    if (list.length === 0) return;
    const ok = await sendText(
      `Update the session todo list to exactly the following by calling the TodoWrite tool now with this argument:\n${JSON.stringify({ todos: list })}`,
    );
    if (ok && sessionId === null) setTodoDraft("new", null);
  };

  const CurrentIcon = STATUS_ICON[current?.status ?? "pending"];

  return (
    <div className="mx-5 mb-1 shrink-0 overflow-hidden rounded-lg border border-tool-border bg-tool-bg text-[0.85rem]">
      <div className="flex items-center gap-2.5 px-3 py-[0.4rem]">
        <button
          onClick={() => show(!open)}
          aria-expanded={open}
          aria-label="Toggle todo panel"
          className="flex shrink-0 items-center gap-1.5 font-semibold text-tool hover:text-foreground"
        >
          <ChevronDown size={14} className={cn("transition-transform", !open && "-rotate-90")} />
          Todos
          <span className="font-normal tabular-nums text-muted-foreground">{`${done}/${shown.length}`}</span>
        </button>
        {!open && shown.length > 0 && (
          <span aria-live="polite" className="flex min-w-0 flex-1 items-center gap-1.5">
            {current ? (
              <>
                <CurrentIcon size={13} className={cn("shrink-0", STATUS_CLASS[current.status])} />
                <span className="truncate">{current.content}</span>
                <Elapsed todo={current} now={now} />
              </>
            ) : (
              <span className="truncate text-muted-foreground">All done</span>
            )}
          </span>
        )}
        {dirty && (
          <button
            onClick={sendTodos}
            disabled={running}
            title={running ? "Stop the current run first" : "Send this list to the agent"}
            className="ml-auto flex shrink-0 items-center gap-1 rounded-md bg-primary px-2 py-[0.15rem] text-[0.78rem] text-primary-foreground hover:bg-primary-hover disabled:cursor-default disabled:opacity-50"
          >
            <Send size={12} /> Send to agent
          </button>
        )}
      </div>
      <div
        role="progressbar"
        aria-label="Todos done"
        aria-valuemin={0}
        aria-valuemax={shown.length}
        aria-valuenow={done}
        className="h-0.5 bg-border"
      >
        <div
          className="h-full bg-tool transition-[width]"
          style={{ width: `${shown.length ? (done / shown.length) * 100 : 0}%` }}
        />
      </div>
      {open && (
        <div>
          <ul className="max-h-[40vh] overflow-y-auto">
            {shown.map((t, i) => {
              const StatusIcon = STATUS_ICON[t.status];
              const dropEdge =
                drag.over === `${TODO_TARGET}${i}` &&
                dragFrom.current !== null &&
                dragFrom.current !== i &&
                dropEdgeClass(dragFrom.current, i);
              return (
                <li
                  key={i}
                  className={cn(
                    "group flex items-center gap-1 border-b border-border/60 px-2 py-1 last:border-b-0",
                    t.status === "in_progress" && "bg-warn/5",
                    dropEdge,
                  )}
                  data-drop-target={`${TODO_TARGET}${i}`}
                >
                  <span
                    aria-hidden="true"
                    onPointerDown={(e) => {
                      dragFrom.current = i;
                      drag.start(e, i);
                    }}
                    className={cn("cursor-grab text-muted-foreground", ROW_EDIT)}
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
                      t.status === "in_progress" && "font-medium",
                      t.status === "completed" && "text-muted-foreground line-through",
                    )}
                  />
                  <Elapsed todo={t} now={now} />
                  <span className={cn("flex shrink-0 items-center gap-1", ROW_EDIT)}>
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
                  </span>
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
