import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent, type WheelEvent } from "react";
import { ArrowLeft } from "lucide-react";
import { api, type Screen } from "@/lib/tauri";
import { computerTiles, type ComputerStatus, type ComputerTile } from "@/lib/computers";
import { framePoint, keyInput, mouseButton, wheelInputs, type ScreenInput } from "@/lib/screen-input";
import { useDesktop } from "@/store";
import { AgentAvatar } from "./AgentAvatar";

const FRAME_POLL_MS = 2000;
const DRIVEN_POLL_MS = 300;
const ESCAPE_TWICE_MS = 2000;

const STATUS_LABEL: Record<ComputerStatus, string> = {
  running: "Working",
  awaiting: "Waiting for you",
  idle: "Idle",
};

const STATUS_DOT: Record<ComputerStatus, string> = {
  running: "animate-pulse bg-emerald-500",
  awaiting: "bg-amber-500",
  idle: "bg-muted-foreground/40",
};

/** The latest screen of a session's computer, polled every `pollMs` while `live` and the window is visible. */
function useScreen(sessionId: string, project: string | undefined, live: boolean, pollMs: number): Screen | null {
  const [screen, setScreen] = useState<Screen | null>(null);
  useEffect(() => {
    let cancelled = false;
    let pending = false;
    const poll = async () => {
      if (pending || document.hidden) return;
      pending = true;
      const frame = await api.screenFrame(sessionId, project).catch(() => null);
      pending = false;
      if (frame && !cancelled) setScreen(frame);
    };
    poll();
    if (!live) return () => void (cancelled = true);
    const timer = setInterval(poll, pollMs);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [sessionId, project, live, pollMs]);
  return screen;
}

function Tile({
  tile,
  project,
  onOpen,
  onTakeOver,
}: {
  tile: ComputerTile;
  project?: string;
  onOpen: () => void;
  onTakeOver: () => void;
}) {
  const screen = useScreen(tile.sessionId, project, tile.status !== "idle", FRAME_POLL_MS);
  return (
    <div className="flex flex-col overflow-hidden rounded-lg border border-border bg-card hover:border-primary/60">
      <button aria-label={`Open computer ${tile.agent.name}`} onClick={onOpen} className="flex flex-col text-left">
        <div className="flex aspect-[16/10] w-full items-center justify-center bg-black/80">
          {screen ? (
            <img src={screen.src} alt={`Screen of ${tile.agent.name}`} className="h-full w-full object-contain" />
          ) : (
            <span className="text-[0.75rem] text-muted-foreground">No screen yet</span>
          )}
        </div>
        <div className="flex items-center gap-2 px-3 py-2">
          <AgentAvatar agent={tile.agent} size={24} />
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="truncate text-[0.85rem] font-medium">{tile.agent.name}</span>
            <span className="truncate text-[0.72rem] text-muted-foreground">
              {tile.workspace}
              {tile.lastAction ? ` - ${tile.lastAction}` : ""}
            </span>
          </div>
          <span className="flex items-center gap-1.5 text-[0.72rem] text-muted-foreground">
            <span className={`h-2 w-2 rounded-full ${STATUS_DOT[tile.status]}`} aria-hidden="true" />
            {STATUS_LABEL[tile.status]}
          </span>
        </div>
      </button>
      <div className="flex justify-end border-t border-border px-3 py-2">
        <button
          onClick={onTakeOver}
          title={`Drive the computer of ${tile.agent.name} yourself`}
          className="rounded-md border border-border-strong px-2.5 py-1 text-[0.78rem] font-medium hover:border-primary/60"
        >
          Take over
        </button>
      </div>
    </div>
  );
}

// The screen of a computer the person took over: clicks, the wheel and keys
// inside it drive that computer, one input after the other, until they hand
// it back with the button or by pressing Esc twice.
function DrivenScreen({ tile, project }: { tile: ComputerTile; project?: string }) {
  const { handBack, setError } = useDesktop();
  const screen = useScreen(tile.sessionId, project, true, DRIVEN_POLL_MS);
  const surface = useRef<HTMLDivElement>(null);
  const image = useRef<HTMLImageElement>(null);
  const inputs = useRef(Promise.resolve());
  const lastEscape = useRef(0);

  useEffect(() => surface.current?.focus(), []);

  const send = (input: ScreenInput) => {
    inputs.current = inputs.current
      .then(() => api.screenInput(tile.sessionId, project, input))
      .then(
        () => undefined,
        (e) => {
          setError(`The computer of ${tile.agent.name} is no longer yours to drive: ${e}`);
          handBack();
        },
      );
  };

  const clickAs = (kind: "click" | "double_click") => (event: MouseEvent) => {
    event.preventDefault();
    surface.current?.focus();
    const shown = image.current;
    if (!shown || !screen) return;
    const frame = { width: screen.width || shown.naturalWidth, height: screen.height || shown.naturalHeight };
    const at = framePoint({ x: event.clientX, y: event.clientY }, shown.getBoundingClientRect(), frame);
    if (at) send({ kind, ...at, button: mouseButton(event.button) });
  };

  const onKeyDown = (event: KeyboardEvent) => {
    event.preventDefault();
    if (event.repeat) return;
    if (event.key === "Escape") {
      const now = Date.now();
      if (now - lastEscape.current < ESCAPE_TWICE_MS) {
        handBack();
        return;
      }
      lastEscape.current = now;
    }
    const input = keyInput(event);
    if (input) send(input);
  };

  const onWheel = (event: WheelEvent) => wheelInputs(event.deltaX, event.deltaY).forEach(send);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="flex items-center gap-3">
        <AgentAvatar agent={tile.agent} size={24} />
        <span aria-live="polite" className="min-w-0 flex-1 truncate text-[0.9rem] font-medium">
          {`You are driving the computer of ${tile.agent.name}`}
          <span className="ml-2 font-normal text-muted-foreground">Esc twice hands it back</span>
        </span>
        <button
          onClick={handBack}
          title="Hand the screen back to the agent"
          className="rounded-md bg-primary px-3 py-1.5 text-[0.82rem] font-medium text-primary-foreground hover:bg-primary/90"
        >
          Hand back
        </button>
      </div>
      <div
        ref={surface}
        tabIndex={0}
        role="application"
        aria-label={`Screen of ${tile.agent.name}, under your control`}
        onKeyDown={onKeyDown}
        onWheel={onWheel}
        className="flex min-h-0 flex-1 items-center justify-center overflow-hidden rounded-lg bg-black/80 outline-none ring-2 ring-primary/60"
      >
        {screen ? (
          <img
            ref={image}
            src={screen.src}
            alt=""
            draggable={false}
            onMouseDown={clickAs("click")}
            onDoubleClick={clickAs("double_click")}
            onContextMenu={(event) => event.preventDefault()}
            className="max-h-full max-w-full cursor-crosshair select-none"
          />
        ) : (
          <span className="text-[0.75rem] text-muted-foreground">No screen yet</span>
        )}
      </div>
    </div>
  );
}

// Every chat that runs on its agent's own computer, side by side: the latest
// screen, what the agent did last and whether it waits for an answer. A
// computer the person took over fills the view until they hand it back.
export function ComputersView() {
  const { agentOf, agents, transcripts, runningIds, projects, openConversation, setCurrentView, drivenId, takeOver } =
    useDesktop();
  const tiles = computerTiles(agentOf, agents, transcripts, runningIds);
  const driven = tiles.find((tile) => tile.sessionId === drivenId);
  const open = (sessionId: string) => {
    setCurrentView("chat");
    openConversation(sessionId);
  };
  return (
    <div id="computers" className="flex min-h-0 flex-1 flex-col overflow-y-auto px-6 py-5">
      <div className="mb-4 flex items-center gap-3">
        <button
          onClick={() => setCurrentView("chat")}
          className="inline-flex items-center gap-1.5 rounded-md px-2 py-[0.45rem] text-[0.85rem] font-medium text-muted-foreground hover:bg-primary/10 hover:text-foreground"
        >
          <ArrowLeft size={15} /> Back
        </button>
        <h2 className="text-[1.05rem] font-semibold">Computers</h2>
      </div>
      {driven ? (
        <DrivenScreen key={driven.sessionId} tile={driven} project={projects[driven.sessionId]} />
      ) : tiles.length === 0 ? (
        <p className="text-[0.85rem] text-muted-foreground">
          No agent is on a computer of its own yet. Give an agent a workspace in its editor, then send it a task.
        </p>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(20rem,1fr))] gap-4">
          {tiles.map((tile) => (
            <Tile
              key={tile.sessionId}
              tile={tile}
              project={projects[tile.sessionId]}
              onOpen={() => open(tile.sessionId)}
              onTakeOver={() => takeOver(tile.sessionId)}
            />
          ))}
        </div>
      )}
    </div>
  );
}
