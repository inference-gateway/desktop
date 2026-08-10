import { RotateCw, Settings } from "lucide-react";
import { useDesktop } from "@/store";
import { ModelSelect } from "./ModelSelect";

export function TopBar() {
  const { versionBadge, showUpdateBanner, updateBannerText, applyUpdates, restartBackend, openSettings } = useDesktop();
  return (
    <header id="top-bar" className="flex flex-wrap items-center gap-3 border-b border-border bg-card px-4 py-[0.6rem]">
      <img src="/logo.png" alt="" width={24} height={24} className="h-6 w-6 shrink-0 rounded-[5px]" />
      <span className="text-[0.95rem] font-[650] tracking-[-0.01em]">Inference Gateway</span>
      <span title="Installed versions" className="shrink-0 text-[0.65rem] text-muted-foreground">
        {versionBadge}
      </span>
      {showUpdateBanner && (
        <button
          title={updateBannerText}
          onClick={() => applyUpdates()}
          className="shrink-0 rounded-full border border-primary bg-primary/10 px-[0.6rem] py-1 text-xs font-medium text-primary hover:bg-primary hover:text-primary-foreground"
        >
          Update available
        </button>
      )}
      <div id="model-controls" className="ml-auto flex items-center gap-2">
        <ModelSelect />
        <button
          title="Restart CLI"
          aria-label="Restart CLI"
          onClick={() => restartBackend(false)}
          className="inline-flex items-center justify-center rounded-md p-[0.35rem] text-muted-foreground hover:bg-secondary hover:text-foreground"
        >
          <RotateCw size={16} />
        </button>
      </div>
      <button
        title="Settings"
        aria-label="Settings"
        onClick={openSettings}
        className="inline-flex items-center justify-center rounded-md p-[0.35rem] text-muted-foreground hover:bg-secondary hover:text-foreground"
      >
        <Settings size={16} />
      </button>
    </header>
  );
}
