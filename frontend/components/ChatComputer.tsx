import { useEffect, useState } from "react";
import { ChevronLeft } from "lucide-react";
import { useComputerFacts } from "@/hooks/useComputerFacts";
import { computerTiles, type ComputerTile } from "@/lib/computers";
import { useChatStream, useDesktop } from "@/store";
import { useAgents } from "@/stores/agents";
import { useSidebar } from "@/stores/sidebar";
import { ComputerCard } from "./ComputerCard";

const INTRODUCTION_MS = 5000;

/** The chats whose computer already had its first showing, so opening one again starts minimized. */
const introduced = new Set<string>();

type View = "introduction" | "shown" | "minimized";

function ShownComputer({
  tile,
  project,
  onMinimize,
}: {
  tile: ComputerTile;
  project?: string;
  onMinimize: () => void;
}) {
  const { takeOver, deleteMachine } = useDesktop();
  const { resources, sandboxes } = useComputerFacts([tile.sessionId]);
  return (
    <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-end py-2 pr-4 [container-type:size]">
      <div className="pointer-events-auto max-h-full w-[min(22rem,60cqw,max(10rem,calc((100cqh_-_8rem)_*_1.6)))] overflow-y-auto rounded-lg shadow-md">
        <ComputerCard
          tile={tile}
          project={project}
          sandbox={sandboxes.has(tile.workspace)}
          resources={resources[tile.sessionId]}
          openLabel={`Minimize computer ${tile.agent.name}`}
          onOpen={onMinimize}
          onTakeOver={() => takeOver(tile.sessionId)}
          onDelete={() => deleteMachine(tile.sessionId)}
        />
      </div>
    </div>
  );
}

function WatchedComputer({ tile, project }: { tile: ComputerTile; project?: string }) {
  const { sessionId } = tile;
  const [view, setView] = useState<View>(() => (introduced.has(sessionId) ? "minimized" : "introduction"));
  useEffect(() => {
    if (view !== "introduction") {
      introduced.add(sessionId);
      return;
    }
    const timer = setTimeout(() => setView("minimized"), INTRODUCTION_MS);
    return () => clearTimeout(timer);
  }, [view, sessionId]);

  if (view !== "minimized") {
    return <ShownComputer tile={tile} project={project} onMinimize={() => setView("minimized")} />;
  }
  return (
    <button
      aria-label={`Show computer ${tile.agent.name}`}
      title={`Show the computer of ${tile.agent.name} on ${tile.workspace}`}
      onClick={() => setView("shown")}
      className="absolute right-0 top-1/2 z-10 flex h-12 w-5 -translate-y-1/2 items-center justify-center rounded-l-md border border-r-0 border-border-strong bg-card text-muted-foreground shadow-md hover:border-primary/60 hover:text-foreground"
    >
      <ChevronLeft size={14} aria-hidden="true" />
    </button>
  );
}

/**
 * The computer of the open chat, inside its transcript: the same tile the Computers view shows. It shows for a
 * moment the first time, then minimizes to a tab on the right edge that brings it back.
 */
export function ChatComputer() {
  const { sessionId, runningIds } = useDesktop();
  const { workspaceOf, agentOf, projects } = useSidebar();
  const { agents } = useAgents();
  const { transcripts } = useChatStream();
  const tile = computerTiles(workspaceOf, agentOf, agents, transcripts, runningIds).find(
    (computer) => computer.sessionId === sessionId,
  );
  if (!tile) return null;
  return <WatchedComputer key={tile.sessionId} tile={tile} project={projects[tile.sessionId]} />;
}
