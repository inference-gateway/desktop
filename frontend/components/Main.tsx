import { Sidebar } from "./Sidebar";
import { Transcript } from "./Transcript";
import { TodoPanel } from "./TodoPanel";
import { Composer } from "./Composer";
import { lazy, Suspense } from "react";
import { useDesktop } from "@/store";

const SettingsView = lazy(() => import("./SettingsView").then((m) => ({ default: m.SettingsView })));
const TimelineView = lazy(() => import("./TimelineView").then((m) => ({ default: m.TimelineView })));
const ObservabilityView = lazy(() => import("./ObservabilityView").then((m) => ({ default: m.ObservabilityView })));

// Content projects show the timeline editor as the workspace with the chat
// docked beside it; code projects keep the plain transcript.
export function Main() {
  const { currentView, currentProject, projectTypes } = useDesktop();
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
  const content = currentProject !== null && projectTypes[currentProject] === "content";
  return (
    <div id="main" className="flex min-h-0 flex-1">
      <Sidebar />
      {content ? (
        <div id="content" className="flex min-w-0 flex-1">
          <Suspense>
            <TimelineView />
          </Suspense>
          <div id="chat-dock" className="flex w-[400px] shrink-0 flex-col border-l border-border">
            <Transcript />
            <TodoPanel />
            <Composer />
          </div>
        </div>
      ) : (
        <div id="content" className="flex min-w-0 flex-1 flex-col">
          <Transcript />
          <TodoPanel />
          <Composer />
        </div>
      )}
    </div>
  );
}
