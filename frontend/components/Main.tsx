import { Sidebar } from "./Sidebar";
import { Transcript } from "./Transcript";
import { TodoPanel } from "./TodoPanel";
import { ActivityPanel } from "./ActivityPanel";
import { Composer } from "./Composer";
import { ResizeHandle } from "./ResizeHandle";
import { lazy, Suspense, useEffect, useState } from "react";
import { useDesktop } from "@/store";
import { clampSidebarWidth, loadSidebarWidth, saveSidebarWidth } from "@/lib/sidebar-width";

const SettingsView = lazy(() => import("./SettingsView").then((m) => ({ default: m.SettingsView })));
const TimelineView = lazy(() => import("./TimelineView").then((m) => ({ default: m.TimelineView })));
const ObservabilityView = lazy(() => import("./ObservabilityView").then((m) => ({ default: m.ObservabilityView })));

const CHAT_DOCK_KEY = "chatDockWidth";
const DEFAULT_CHAT_DOCK_WIDTH = 400;

// Content projects show the timeline editor as the workspace with the chat
// docked beside it; code projects keep the plain transcript. Both side
// panels collapse and resize; the sidebar starts collapsed for content
// projects so the editor gets the room.
export function Main() {
  const { currentView, currentProject, projectTypes, sidebarOpen, setSidebarOpen, chatOpen } = useDesktop();
  const content = currentProject !== null && projectTypes[currentProject] === "content";
  useEffect(() => setSidebarOpen(!content), [content, setSidebarOpen]);
  if (currentView === "settings") {
    return (
      <div id="main" className="flex min-h-0 flex-1">
        <Suspense>
          <SettingsView />
        </Suspense>
      </div>
    );
  }
  if (currentView === "observability") {
    return (
      <div id="main" className="flex min-h-0 flex-1">
        <Suspense>
          <ObservabilityView />
        </Suspense>
      </div>
    );
  }
  return (
    <div id="main" className="flex min-h-0 flex-1">
      {sidebarOpen && <Sidebar />}
      {content ? (
        <div id="content" className="flex min-w-0 flex-1">
          <Suspense>
            <TimelineView />
          </Suspense>
          {chatOpen && <ChatDock />}
        </div>
      ) : (
        <div id="content" className="flex min-w-0 flex-1 flex-col">
          <ActivityPanel />
          <Transcript />
          <TodoPanel />
          <Composer />
        </div>
      )}
    </div>
  );
}

function ChatDock() {
  const [width, setWidth] = useState(() => loadSidebarWidth(window.innerWidth, CHAT_DOCK_KEY, DEFAULT_CHAT_DOCK_WIDTH));
  return (
    <div id="chat-dock" style={{ width }} className="relative flex shrink-0 flex-col border-l border-border">
      <ResizeHandle
        edge="left"
        width={width}
        clamp={(w) => clampSidebarWidth(w, window.innerWidth)}
        onChange={setWidth}
        onEnd={() => saveSidebarWidth(width, CHAT_DOCK_KEY)}
        onReset={() => {
          setWidth(DEFAULT_CHAT_DOCK_WIDTH);
          saveSidebarWidth(DEFAULT_CHAT_DOCK_WIDTH, CHAT_DOCK_KEY);
        }}
        title="Drag to resize chat (double-click resets)"
      />
      <ActivityPanel />
      <Transcript />
      <TodoPanel />
      <Composer />
    </div>
  );
}
