import { listen } from "@tauri-apps/api/event";
import { cn } from "@/lib/utils";
import { api, type BrowserUseStatus } from "@/lib/tauri";
import { useEffect, useState } from "react";

export function useBrowserUseStatus() {
  const [status, setStatus] = useState<BrowserUseStatus | null>(null);
  useEffect(() => {
    api
      .browserUseStatus()
      .then(setStatus)
      .catch(() => {});
    const unlisten = listen<BrowserUseStatus>("browser-bridge", (e) => setStatus(e.payload));
    return () => {
      unlisten.then((f) => f());
    };
  }, []);
  return status;
}

export function BrowserBridgeDot() {
  const status = useBrowserUseStatus();
  if (!status?.enabled) return null;
  const title = status.connected
    ? "Browser extension connected"
    : `Browser extension not connected - set port ${status.port} and the token from Settings in the opentask extension`;
  return (
    <div
      id="browser-bridge"
      title={title}
      className="mx-auto mb-[0.45rem] flex max-w-[52rem] items-center gap-2 px-1 text-[0.7rem] text-muted-foreground"
    >
      <span
        className={cn(
          "h-[0.45rem] w-[0.45rem] shrink-0 rounded-full",
          status.connected ? "bg-emerald-500" : "bg-muted-foreground",
        )}
      />
      Browser {status.connected ? "connected" : "disconnected"}
    </div>
  );
}
