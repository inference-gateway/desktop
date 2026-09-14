import { listen } from "@tauri-apps/api/event";
import { cn } from "@/lib/utils";
import { Globe } from "lucide-react";
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
  const label = status.connected
    ? "Browser extension connected"
    : `Browser extension not connected - set port ${status.port} and the token from Settings in the opentask extension`;
  return (
    <span
      id="browser-bridge"
      role="img"
      aria-label={label}
      title={label}
      className={cn(
        "inline-flex size-7 items-center justify-center rounded-full",
        status.connected ? "text-emerald-500" : "text-muted-foreground opacity-50",
      )}
    >
      <Globe size={16} aria-hidden="true" />
    </span>
  );
}
