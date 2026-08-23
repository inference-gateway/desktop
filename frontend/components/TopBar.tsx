import { useState } from "react";
import { ChartColumn, LoaderCircle, RotateCw, Settings, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useDesktop } from "@/store";
import { ModelSelect } from "./ModelSelect";

export function TopBar() {
  const {
    versionBadge,
    showUpdateBanner,
    updateBannerText,
    applyUpdates,
    restartBackend,
    openSettings,
    openObservability,
    autoMode,
    setAutoMode,
  } = useDesktop();
  const [isUpdating, setIsUpdating] = useState(false);

  const handleUpdate = async () => {
    if (isUpdating) return;
    setIsUpdating(true);
    try {
      await applyUpdates();
    } finally {
      setIsUpdating(false);
    }
  };

  return (
    <header id="top-bar" className="flex flex-wrap items-center gap-3 border-b border-border bg-card px-4 py-[0.6rem]">
      <img src="/logo.png" alt="" width={24} height={24} className="h-6 w-6 shrink-0 rounded-[5px]" />
      <span className="text-[0.95rem] font-[650] tracking-[-0.01em]">Inference Gateway</span>
      <span title="Installed versions" className="shrink-0 text-[0.65rem] text-muted-foreground">
        {versionBadge}
      </span>
      {showUpdateBanner && (
        <button
          type="button"
          title={isUpdating ? "Update in progress - see status below" : updateBannerText}
          aria-busy={isUpdating}
          disabled={isUpdating}
          onClick={handleUpdate}
          className="flex shrink-0 items-center gap-1 rounded-full border border-primary bg-primary/10 px-[0.6rem] py-1 text-xs font-medium text-primary hover:bg-primary hover:text-primary-foreground disabled:cursor-wait disabled:opacity-60"
        >
          {isUpdating && <LoaderCircle size={12} className="animate-spin" aria-hidden="true" />}
          {isUpdating ? "Updating..." : "Update available"}
        </button>
      )}
      <div id="model-controls" className="ml-auto flex items-center gap-2">
        <Button
          variant={autoMode ? "default" : "outline"}
          size="sm"
          aria-label="Auto mode"
          aria-pressed={autoMode}
          title={
            autoMode
              ? "Auto mode is on - new runs do not ask for tool approval"
              : "Auto mode is off - new runs ask before protected tool actions"
          }
          onClick={() => setAutoMode(!autoMode)}
          className="h-8 gap-1.5 px-2.5"
        >
          <Zap size={14} />
          Auto mode
          <span
            aria-hidden="true"
            className="rounded-full border border-current/25 px-1.5 py-0.5 text-[0.62rem] font-bold uppercase leading-none"
          >
            {autoMode ? "On" : "Off"}
          </span>
        </Button>
        <ModelSelect />
        <Button
          variant="ghost"
          size="icon-sm"
          title="Restart CLI"
          aria-label="Restart CLI"
          onClick={() => restartBackend(false)}
          className="text-muted-foreground"
        >
          <RotateCw size={16} />
        </Button>
      </div>
      <Button
        variant="ghost"
        size="icon-sm"
        title="Observability"
        aria-label="Observability"
        onClick={openObservability}
        className="text-muted-foreground"
      >
        <ChartColumn size={16} />
      </Button>
      <Button
        variant="ghost"
        size="icon-sm"
        title="Settings"
        aria-label="Settings"
        onClick={openSettings}
        className="text-muted-foreground"
      >
        <Settings size={16} />
      </Button>
    </header>
  );
}
