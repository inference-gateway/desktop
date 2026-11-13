import { useState } from "react";
import { Trash2 } from "lucide-react";
import { useScreen, WATCHED_POLL_MS } from "@/hooks/useScreen";
import { computerSummary, type ComputerStatus, type ComputerTile } from "@/lib/computers";
import type { ComputerResources } from "@/lib/tauri";
import { AgentAvatar } from "./AgentAvatar";

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

/**
 * One computer as a tile: its latest screen, whose it is, what it is and the buttons to take it over or delete
 * it. A click on the tile runs `onOpen`, named by `openLabel` where the tile does not open a chat.
 */
export function ComputerCard({
  tile,
  project,
  sandbox,
  resources,
  openLabel,
  onOpen,
  onTakeOver,
  onDelete,
}: {
  tile: ComputerTile;
  project?: string;
  sandbox: boolean;
  resources?: ComputerResources;
  openLabel?: string;
  onOpen: () => void;
  onTakeOver: () => void;
  onDelete: () => void;
}) {
  const screen = useScreen(tile.sessionId, project, tile.status !== "idle", WATCHED_POLL_MS);
  const [deleteArmed, setDeleteArmed] = useState(false);
  return (
    <div className="flex flex-col overflow-hidden rounded-lg border border-border bg-card hover:border-primary/60">
      <button
        aria-label={openLabel ?? `Open computer ${tile.agent.name}`}
        onClick={onOpen}
        className="flex flex-col text-left"
      >
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
        <span
          title={
            sandbox
              ? "A pod of its own in a cluster. What it shows is what this computer started with."
              : "Not a sandbox - the agent works on a real system."
          }
          className="truncate px-3 pb-2 text-[0.72rem] text-muted-foreground"
        >
          {computerSummary(sandbox, resources)}
        </span>
      </button>
      <div className="flex justify-end gap-2 border-t border-border px-3 py-2">
        <button
          aria-label={`Delete computer ${tile.agent.name}`}
          title={
            deleteArmed
              ? "Click again to delete this computer and the chat that lives on it"
              : `Delete the computer of ${tile.agent.name}`
          }
          onClick={() => {
            if (deleteArmed) onDelete();
            setDeleteArmed(!deleteArmed);
          }}
          onBlur={() => setDeleteArmed(false)}
          className={`rounded-md border px-2 py-1 ${
            deleteArmed ? "border-destructive text-destructive" : "border-border-strong hover:border-destructive/60"
          }`}
        >
          <Trash2 size={14} />
        </button>
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
