import { Sidebar } from "./Sidebar";
import { Transcript } from "./Transcript";
import { Composer } from "./Composer";
import { SettingsView } from "./SettingsView";
import { useDesktop } from "@/store";

export function Main() {
  const { currentView } = useDesktop();
  if (currentView === "settings") {
    return (
      <div id="main" className="flex min-h-0 flex-1">
        <SettingsView />
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
