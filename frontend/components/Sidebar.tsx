import { useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { Sparkles } from "lucide-react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { Button } from "@/components/ui/button";
import { ChatList } from "./ChatList";
import { clampSidebarWidth, DEFAULT_SIDEBAR_WIDTH, loadSidebarWidth, saveSidebarWidth } from "@/lib/sidebar-width";

/** Prompt to create a new project shown below the chat list */
export function Sidebar() {
  const { newChat, projectNames, initSelecting, startInitSelection, cancelInitSelection, initAllRunning } =
    useDesktop();
  const [width, setWidth] = useState(() => loadSidebarWidth(window.innerWidth));
  const [dragging, setDragging] = useState(false);
  const drag = useRef<{ x: number; w: number } | null>(null);

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    drag.current = { x: e.clientX, w: width };
    setDragging(true);
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag.current) return;
    setWidth(clampSidebarWidth(drag.current.w + e.clientX - drag.current.x, window.innerWidth));
  };
  const endDrag = () => {
    if (!drag.current) return;
    drag.current = null;
    setDragging(false);
    saveSidebarWidth(width);
  };
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
      <div
        aria-hidden="true"
        title="Drag to resize sidebar (double-click resets)"
        className="absolute inset-y-0 right-0 z-10 w-1.5 cursor-col-resize touch-none hover:bg-primary/30"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onDoubleClick={resetWidth}
      />
    </aside>
  );
}
