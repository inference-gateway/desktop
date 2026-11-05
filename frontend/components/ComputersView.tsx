import { useEffect, useState } from "react";
import { ArrowLeft } from "lucide-react";
import { api } from "@/lib/tauri";
import { computerTiles, type ComputerStatus, type ComputerTile } from "@/lib/computers";
import { useDesktop } from "@/store";
import { AgentAvatar } from "./AgentAvatar";

const FRAME_POLL_MS = 2000;

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

/** The latest screen of a session's computer, polled while `live` and the window is visible. */
function useScreen(sessionId: string, project: string | undefined, live: boolean): string | null {
  const [screen, setScreen] = useState<string | null>(null);
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
    const timer = setInterval(poll, FRAME_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [sessionId, project, live]);
  return screen;
}

function Tile({ tile, project, onOpen }: { tile: ComputerTile; project?: string; onOpen: () => void }) {
  const screen = useScreen(tile.sessionId, project, tile.status !== "idle");
  return (
    <button
      aria-label={`Open computer ${tile.agent.name}`}
      onClick={onOpen}
      className="flex flex-col overflow-hidden rounded-lg border border-border bg-card text-left hover:border-primary/60"
    >
      <div className="flex aspect-[16/10] w-full items-center justify-center bg-black/80">
        {screen ? (
          <img src={screen} alt={`Screen of ${tile.agent.name}`} className="h-full w-full object-contain" />
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
  );
}

// Every chat that runs on its agent's own computer, side by side: the latest
// screen, what the agent did last and whether it waits for an answer.
export function ComputersView() {
  const { agentOf, agents, transcripts, runningIds, projects, openConversation, setCurrentView } = useDesktop();
  const tiles = computerTiles(agentOf, agents, transcripts, runningIds);
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
      {tiles.length === 0 ? (
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
            />
          ))}
        </div>
      )}
    </div>
  );
}
