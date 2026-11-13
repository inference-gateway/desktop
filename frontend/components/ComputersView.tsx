import { useEffect, useRef, type KeyboardEvent, type MouseEvent, type WheelEvent } from "react";
import { ArrowLeft } from "lucide-react";
import { api } from "@/lib/tauri";
import { computerTiles, type ComputerTile } from "@/lib/computers";
import {
  framePoint,
  isUnsupportedInput,
  keyInput,
  latestOnly,
  mouseButton,
  wheelInputs,
  type ScreenInput,
} from "@/lib/screen-input";
import { useComputerFacts } from "@/hooks/useComputerFacts";
import { useScreen } from "@/hooks/useScreen";
import { useChatStream, useDesktop } from "@/store";
import { useAgents } from "@/stores/agents";
import { useSidebar } from "@/stores/sidebar";
import { useStatusActions } from "@/stores/status";
import { AgentAvatar } from "./AgentAvatar";
import { ComputerCard } from "./ComputerCard";

const DRIVEN_POLL_MS = 300;
const ESCAPE_TWICE_MS = 2000;

// The screen of a computer the person took over: clicks, the wheel and keys
// inside it drive that computer, one input after the other, until they hand
// it back with the button or by pressing Esc twice.
function DrivenScreen({ tile, project }: { tile: ComputerTile; project?: string }) {
  const { setError } = useStatusActions();
  const { handBack } = useDesktop();
  const screen = useScreen(tile.sessionId, project, true, DRIVEN_POLL_MS);
  const surface = useRef<HTMLDivElement>(null);
  const image = useRef<HTMLImageElement>(null);
  const inputs = useRef(Promise.resolve());
  const lastEscape = useRef(0);
  const pointerMoves = useRef(latestOnly<{ x: number; y: number }>());
  const canPress = useRef(true);
  const stopAwaitingRelease = useRef(() => {});

  useEffect(() => surface.current?.focus(), []);
  useEffect(() => () => stopAwaitingRelease.current(), []);

  const drive = (input: ScreenInput) => api.screenInput(tile.sessionId, project, input);

  const whenItsTurnComes = (driveNext: () => Promise<unknown> | undefined) => {
    inputs.current = inputs.current.then(driveNext).then(
      () => undefined,
      (e) => {
        setError(`The computer of ${tile.agent.name} is no longer yours to drive: ${e}`);
        handBack();
      },
    );
  };

  const sendWhenItsTurnComes = (nextInput: () => ScreenInput | null) =>
    whenItsTurnComes(() => {
      const input = nextInput();
      return input ? drive(input) : undefined;
    });

  const send = (input: ScreenInput) => sendWhenItsTurnComes(() => input);

  const pointOnScreen = (event: { clientX: number; clientY: number }) => {
    const shown = image.current;
    if (!shown || !screen) return null;
    const frame = { width: screen.width || shown.naturalWidth, height: screen.height || shown.naturalHeight };
    return framePoint({ x: event.clientX, y: event.clientY }, shown.getBoundingClientRect(), frame);
  };

  const pressOrClick = async (at: { x: number; y: number }, button: "left" | "right" | "middle") => {
    if (canPress.current) {
      try {
        await drive({ kind: "mouse_down", ...at, button });
        return;
      } catch (e) {
        if (!isUnsupportedInput(e)) throw e;
        canPress.current = false;
      }
    }
    await drive({ kind: "click", ...at, button });
  };

  const press = (event: MouseEvent) => {
    event.preventDefault();
    surface.current?.focus();
    const at = pointOnScreen(event);
    if (!at) return;
    const button = mouseButton(event.button);
    whenItsTurnComes(() => pressOrClick(at, button));

    const release = (up: globalThis.MouseEvent) => {
      if (mouseButton(up.button) !== button) return;
      stopAwaitingRelease.current();
      const end = pointOnScreen(up) ?? at;
      sendWhenItsTurnComes(() => (canPress.current ? { kind: "mouse_up", ...end, button } : null));
    };
    stopAwaitingRelease.current();
    stopAwaitingRelease.current = () => window.removeEventListener("mouseup", release);
    window.addEventListener("mouseup", release);
  };

  const movePointer = (event: MouseEvent) => {
    const at = pointOnScreen(event);
    if (!at) return;
    const noMoveWaiting = pointerMoves.current.put(at);
    if (noMoveWaiting) {
      sendWhenItsTurnComes(() => {
        const latest = pointerMoves.current.take();
        return latest ? { kind: "move", ...latest } : null;
      });
    }
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
            onMouseDown={press}
            onMouseMove={movePointer}
            onContextMenu={(event) => event.preventDefault()}
            className="max-h-full max-w-full cursor-default select-none"
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
  const { workspaceOf, agentOf, projects } = useSidebar();
  const { agents } = useAgents();
  const { transcripts } = useChatStream();
  const { runningIds, openConversation, setCurrentView, drivenId, takeOver, deleteMachine } = useDesktop();
  const tiles = computerTiles(workspaceOf, agentOf, agents, transcripts, runningIds);
  const driven = tiles.find((tile) => tile.sessionId === drivenId);
  const { resources, sandboxes } = useComputerFacts(tiles.map((tile) => tile.sessionId));
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
          No chat runs on a workspace yet. Give an agent a workspace in its editor and send it a task, or move a chat to
          a workspace from its composer.
        </p>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(20rem,1fr))] gap-4">
          {tiles.map((tile) => (
            <ComputerCard
              key={tile.sessionId}
              tile={tile}
              project={projects[tile.sessionId]}
              sandbox={sandboxes.has(tile.workspace)}
              resources={resources[tile.sessionId]}
              onOpen={() => open(tile.sessionId)}
              onTakeOver={() => takeOver(tile.sessionId)}
              onDelete={() => deleteMachine(tile.sessionId)}
            />
          ))}
        </div>
      )}
    </div>
  );
}
