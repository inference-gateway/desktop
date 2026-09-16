import { useState } from "react";
import { Sparkles } from "lucide-react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { Button } from "@/components/ui/button";
import { ChatList } from "./ChatList";
import { ResizeHandle } from "./ResizeHandle";
import { clampSidebarWidth, DEFAULT_SIDEBAR_WIDTH, loadSidebarWidth, saveSidebarWidth } from "@/lib/sidebar-width";

export function Sidebar() {
  const { newChat, projectNames, initSelecting, startInitSelection, cancelInitSelection, initAllRunning } =
    useDesktop();
  const [width, setWidth] = useState(() => loadSidebarWidth(window.innerWidth));
  const [dragging, setDragging] = useState(false);
  const resetWidth = () => {
    setWidth(DEFAULT_SIDEBAR_WIDTH);
    saveSidebarWidth(DEFAULT_SIDEBAR_WIDTH);
  };
  return (
    <aside
      id="sidebar"
      style={{ width }}
      className={cn(
        "relative flex shrink-0 flex-col gap-2 overflow-hidden border-r border-border bg-secondary p-3",
        dragging && "select-none",
      )}
    >
      <div className="flex gap-2">
        <Button onClick={newChat} title="New chat (⌘N / Ctrl+N)" className="flex-1 text-[0.85rem]">
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
      <ResizeHandle
        edge="right"
        width={width}
        clamp={(w) => clampSidebarWidth(w, window.innerWidth)}
        onChange={setWidth}
        onEnd={() => saveSidebarWidth(width)}
        onReset={resetWidth}
        onDragging={setDragging}
        title="Drag to resize sidebar (double-click resets)"
      />
    </aside>
  );
}
