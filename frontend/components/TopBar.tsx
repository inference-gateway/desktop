import { useEffect, useRef, useState } from "react";
import {
  ChartColumn,
  Circle,
  LoaderCircle,
  PanelLeftClose,
  PanelLeftOpen,
  PanelRightClose,
  PanelRightOpen,
  RotateCw,
  Settings,
  Square,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { emit } from "@tauri-apps/api/event";
import { api, isMacOS, screenRecordKeep } from "@/lib/tauri";
import { autoGrow } from "@/lib/textarea";
import { useDesktop } from "@/store";
import { ModelSelect } from "./ModelSelect";

const RECORDING_TITLE = "Record your screen and key presses so the agent can turn the workflow into a skill";
const MAX_RECORD_SECS = 180;

export function TopBar() {
  const {
    versionBadge,
    showUpdateBanner,
    updateBannerText,
    applyUpdates,
    restartBackend,
    openSettings,
    openObservability,
    composerRef,
    setError,
    sidebarOpen,
    setSidebarOpen,
    chatOpen,
    setChatOpen,
    currentView,
    currentProject,
    projectTypes,
  } = useDesktop();
  const content = currentView === "chat" && currentProject !== null && projectTypes[currentProject] === "content";
  const [isUpdating, setIsUpdating] = useState(false);
  const [recording, setRecording] = useState(false);
  const [elapsed, setElapsed] = useState(0);
  const startedAtRef = useRef(0);
  const stopRef = useRef<() => void>(() => {});

  useEffect(() => {
    if (!isMacOS) return;
    api
      .screenRecordingStatus()
      .then(setRecording)
      .catch(() => {});
  }, []);

  useEffect(() => {
    emit("screen-recording", { recording }).catch(() => {});
    if (!recording) return;
    startedAtRef.current = Date.now();
    setElapsed(0);
    const timer = setInterval(() => {
      const secs = Math.floor((Date.now() - startedAtRef.current) / 1000);
      setElapsed(secs);
      if (secs >= MAX_RECORD_SECS) stopRef.current();
    }, 1000);
    return () => clearInterval(timer);
  }, [recording]);

  const toggleRecording = async () => {
    if (recording) {
      try {
        const dir = await api.stopScreenRecording();
        setRecording(false);
        const el = composerRef.current;
        if (el) {
          const ref = `[Attached screen recording: ${dir}]\nIt contains frames/*.jpg captured at 1 fps and events.jsonl with timestamped key presses and clicks.`;
          const ask =
            "Create a skill for this workflow. Use /skill-creator. Before writing anything, ask me in a plain message (not a tool call) whether to store it globally in ~/.infer/skills/<name>/SKILL.md (default) or in this project's .agents/skills/<name>/SKILL.md.";
          const text = el.value.trim();
          el.value = text ? `${ref}\n\n${text}` : `${ref}\n\n${ask}`;
          autoGrow(el);
          el.focus();
        }
      } catch (e) {
        setError(`Failed to stop recording: ${e}`);
      }
    } else {
      try {
        await api.startScreenRecording(screenRecordKeep());
        setRecording(true);
      } catch (e) {
        setError(`Failed to start recording: ${e}`);
      }
    }
  };

  stopRef.current = () => {
    if (recording) toggleRecording();
  };

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
      <Button
        variant="ghost"
        size="icon-sm"
        title={sidebarOpen ? "Hide sidebar" : "Show sidebar"}
        aria-label="Toggle sidebar"
        aria-pressed={sidebarOpen}
        onClick={() => setSidebarOpen(!sidebarOpen)}
        className="text-muted-foreground"
      >
        {sidebarOpen ? <PanelLeftClose size={16} /> : <PanelLeftOpen size={16} />}
      </Button>
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
      {isMacOS && (
        <>
          <Button
            variant="ghost"
            size="icon-sm"
            title={recording ? "Stop recording - key presses are captured in the events log" : RECORDING_TITLE}
            aria-label={recording ? "Stop recording" : "Record workflow"}
            onClick={toggleRecording}
            className={recording ? "text-destructive" : "text-muted-foreground"}
          >
            {recording ? <Square size={16} /> : <Circle size={16} />}
          </Button>
          {recording && (
            <span aria-live="polite" className="flex items-center gap-1.5 text-[0.7rem] font-medium text-destructive">
              <span className="h-2 w-2 animate-pulse rounded-full bg-destructive" aria-hidden="true" />
              {`${String(Math.floor(elapsed / 60)).padStart(2, "0")}:${String(elapsed % 60).padStart(2, "0")}`}
            </span>
          )}
        </>
      )}
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
      {content && (
        <Button
          variant="ghost"
          size="icon-sm"
          title={chatOpen ? "Hide chat" : "Show chat"}
          aria-label="Toggle chat"
          aria-pressed={chatOpen}
          onClick={() => setChatOpen(!chatOpen)}
          className="text-muted-foreground"
        >
          {chatOpen ? <PanelRightClose size={16} /> : <PanelRightOpen size={16} />}
        </Button>
      )}
    </header>
  );
}
