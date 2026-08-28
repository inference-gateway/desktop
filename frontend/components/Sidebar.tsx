import { Sparkles } from "lucide-react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { Button } from "@/components/ui/button";
import { ChatList } from "./ChatList";

/** Prompt to create a new project shown below the chat list */
export function Sidebar() {
  const { newChat, projectNames, initSelecting, startInitSelection, cancelInitSelection, initAllRunning } =
    useDesktop();
  return (
    <aside
      id="sidebar"
      className="flex w-[320px] shrink-0 flex-col gap-2 overflow-hidden border-r border-border bg-secondary p-3"
    >
      <div className="flex gap-2">
        <Button
          onClick={newChat}
          title="New chat (⌘N / Ctrl+N)"
          className="flex-1 text-[0.85rem]"
        >
          + New chat
        </Button>
        <Button
          size="icon"
          aria-label="Init all projects"
          aria-pressed={initSelecting}
          title={initSelecting ? "Cancel project selection" : "Init all projects"}
          disabled={projectNames.length === 0 || initAllRunning}
          onClick={() => (initSelecting ? cancelInitSelection() : startInitSelection())}
          className={cn(initSelecting && "ring-2 ring-primary")}
        >
          <Sparkles />
        </Button>
      </div>
      <ChatList />
    </aside>
  );
}
