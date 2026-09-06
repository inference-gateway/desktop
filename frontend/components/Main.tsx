import { Sidebar } from "./Sidebar";
import { Transcript } from "./Transcript";
import { Composer } from "./Composer";
import { lazy, Suspense } from "react";
import { useDesktop } from "@/store";

const SettingsView = lazy(() => import("./SettingsView").then((m) => ({ default: m.SettingsView })));
const TimelineView = lazy(() => import("./TimelineView").then((m) => ({ default: m.TimelineView })));
const ObservabilityView = lazy(() => import("./ObservabilityView").then((m) => ({ default: m.ObservabilityView })));

export function Main() {
  const { currentView } = useDesktop();
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
  if (currentView === "timeline") {
    return (
      <div id="main" className="flex min-h-0 flex-1">
        <Suspense>
          <TimelineView />
        </Suspense>
      </div>
    );
  }
  return (
    <div id="main" className="flex min-h-0 flex-1">
      <Sidebar />
      <div id="content" className="flex min-w-0 flex-1 flex-col">
        <Transcript />
        <Composer />
      </div>
    </div>
  );
}
