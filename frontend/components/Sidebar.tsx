import { useEffect, useState } from "react";
import { Bot, Clapperboard, Code, LayoutList, MessageSquarePlus, Radio } from "lucide-react";
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
    if (!mixedTypes) setFilter((f) => (f === "agents" ? f : null));
  }, [mixedTypes, setFilter]);
  const filters = [
    { id: null, label: "Filter all", title: "Show everything", icon: <LayoutList /> },
    ...(mixedTypes
      ? [
          { id: "code" as const, label: "Filter code projects", title: "Show only code projects", icon: <Code /> },
          {
            id: "content" as const,
            label: "Filter content projects",
            title: "Show only content projects",
            icon: <Clapperboard />,
          },
        ]
      : []),
    { id: "agents" as const, label: "Filter agents", title: "Show your agents", icon: <Bot /> },
  ];
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
          variant="outline"
          aria-label="Broadcast to projects"
          aria-pressed={initSelecting}
          title={initSelecting ? "Cancel project selection" : "Broadcast to projects"}
          disabled={projectNames.length === 0 || initAllRunning}
          onClick={() => (initSelecting ? cancelInitSelection() : startInitSelection())}
          className={cn(initSelecting && "ring-2 ring-primary")}
        >
          <Radio />
        </Button>
      </div>
      <div role="tablist" aria-label="Sidebar filters" className="flex gap-2">
        {filters.map((f) => (
          <Button
            key={f.id ?? "all"}
            role="tab"
            size="icon"
            variant="outline"
            aria-label={f.label}
            aria-pressed={filter === f.id}
            aria-selected={filter === f.id}
            title={f.title}
            onClick={() => setFilter(f.id)}
            className={cn(filter === f.id && "ring-2 ring-primary")}
          >
            {f.icon}
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
