import { listen } from "@tauri-apps/api/event";
import { Globe } from "lucide-react";
import { api, type BrowserUseStatus } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

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

const dot = "inline-flex size-7 items-center justify-center rounded-full";

export function BrowserBridgeDot() {
  const status = useBrowserUseStatus();
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!status?.connected) setConfirming(false);
  }, [status?.connected]);

  if (!status?.enabled) return null;

  if (!status.connected) {
    const label = `Browser extension not connected - set port ${status.port} and the token from Settings in the opentask extension`;
    return (
      <span
        id="browser-bridge"
        role="img"
        aria-label={label}
        title={label}
        className={cn(dot, "text-muted-foreground opacity-50")}
      >
        <Globe size={16} aria-hidden="true" />
      </span>
    );
  }

  const disconnect = () => {
    setError("");
    api
      .setBrowserUseEnabled(false)
      .then(() => setConfirming(false))
      .catch((e) => setError(String(e)));
  };

  return (
    <>
      <button
        id="browser-bridge"
        type="button"
        aria-label="Disconnect opentask extension"
        title="Browser extension connected - click to disconnect"
        onClick={() => {
          setError("");
          setConfirming(true);
        }}
        className={cn(dot, "text-emerald-500 hover:bg-secondary")}
      >
        <Globe size={16} aria-hidden="true" />
      </button>
      <Dialog open={confirming} onOpenChange={setConfirming}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>Disconnect from the opentask extension?</DialogTitle>
            <DialogDescription>
              This turns off Browser Use and the extension cannot connect. Turn it back on in Settings &gt; General to
              reconnect.
            </DialogDescription>
          </DialogHeader>
          {error && (
            <p role="alert" className="text-[0.75rem] text-destructive">
              {error}
            </p>
          )}
          <DialogFooter>
            <Button variant="outline" size="sm" onClick={() => setConfirming(false)}>
              Cancel
            </Button>
            <Button variant="destructive" size="sm" onClick={disconnect}>
              Disconnect
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
