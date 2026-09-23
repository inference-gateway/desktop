import { useEffect, useState } from "react";
import { Clapperboard, Code, MessageSquarePlus, Radio } from "lucide-react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { Button } from "@/components/ui/button";
import { ChatList } from "./ChatList";
import { ResizeHandle } from "./ResizeHandle";
import { clampSidebarWidth, DEFAULT_SIDEBAR_WIDTH, loadSidebarWidth, saveSidebarWidth } from "@/lib/sidebar-width";

export function Sidebar() {
  const {
    newChat,
    projectNames,
    projectTypes,
    projectFilter: filter,
    setProjectFilter: setFilter,
    initSelecting,
    startInitSelection,
    cancelInitSelection,
    initAllRunning,
  } = useDesktop();
  const [width, setWidth] = useState(() => loadSidebarWidth(window.innerWidth));
  const [dragging, setDragging] = useState(false);
  const mixedTypes = new Set(projectNames.map((n) => projectTypes[n] ?? "code")).size > 1;
  useEffect(() => {
    if (!mixedTypes) setFilter(null);
  }, [mixedTypes, setFilter]);
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
        <Button size="icon" variant="outline" aria-label="New chat" title="New chat (⌘N / Ctrl+N)" onClick={newChat}>
          <MessageSquarePlus />
        </Button>
        <Button
          size="icon"
          aria-label="Broadcast to projects"
          aria-pressed={initSelecting}
          title={initSelecting ? "Cancel project selection" : "Broadcast to projects"}
          disabled={projectNames.length === 0 || initAllRunning}
          onClick={() => (initSelecting ? cancelInitSelection() : startInitSelection())}
          className={cn(initSelecting && "ring-2 ring-primary")}
        >
          <Radio />
        </Button>
        {mixedTypes &&
          (["code", "content"] as const).map((t) => (
            <Button
              key={t}
              size="icon"
              variant="outline"
              aria-label={`Filter ${t} projects`}
              aria-pressed={filter === t}
              title={filter === t ? "Show all projects" : `Show only ${t} projects`}
              onClick={() => setFilter(filter === t ? null : t)}
              className={cn(filter === t && "ring-2 ring-primary")}
            >
              {t === "code" ? <Code /> : <Clapperboard />}
            </Button>
          ))}
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
