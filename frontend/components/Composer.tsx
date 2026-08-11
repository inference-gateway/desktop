import { ArrowUp, Folder, Mic, Square, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { StatusBar } from "./StatusBar";
import { TokenReadout } from "./TokenReadout";
import { SnippetBar } from "./SnippetBar";
import { useVoiceInput } from "@/hooks/useVoiceInput";
import { autoGrow } from "@/lib/textarea";
import { useRef } from "react";

const ROUND = "inline-flex h-[2.2rem] w-[2.2rem] items-center justify-center rounded-full";

export function Composer() {
  const { composerRef, enabled, running, send, cancel, setStatus, setError, history, activeProject, setActiveProject, currentProject } = useDesktop();
  const voice = useVoiceInput({ textareaRef: composerRef, running, setStatus, setError });
  const cursorRef = useRef(-1);
  const draftRef = useRef("");

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget;
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      cursorRef.current = -1;
      draftRef.current = "";
      send();
      return;
    }
    if (e.key === "ArrowUp" && history.length > 0) {
      e.preventDefault();
      if (cursorRef.current === -1) {
        draftRef.current = el.value;
        cursorRef.current = history.length - 1;
      } else if (cursorRef.current > 0) {
        cursorRef.current--;
      } else {
        return;
      }
      el.value = history[cursorRef.current];
      autoGrow(el);
      return;
    }
    if (e.key === "ArrowDown" && history.length > 0 && cursorRef.current !== -1) {
      e.preventDefault();
      if (cursorRef.current < history.length - 1) {
        cursorRef.current++;
        el.value = history[cursorRef.current];
      } else {
        cursorRef.current = -1;
        el.value = draftRef.current;
      }
      autoGrow(el);
      return;
    }
  };

  return (
    <div id="input-area" className="border-t border-border bg-card px-4 pb-4 pt-[0.6rem]">
      <StatusBar />
      <TokenReadout />
      {currentProject && (
        <div className="mx-auto -mb-3 flex w-[calc(100%-1.5rem)] max-w-[50rem] items-center gap-2 rounded-t-[1rem] bg-secondary px-4 pb-4 pt-2 text-[0.85rem] text-muted-foreground">
          <Folder size={14} className="shrink-0" />
          <span className="min-w-0 flex-1 truncate">{currentProject}</span>
          {currentProject === activeProject && (
            <button
              aria-label="Leave project context"
              title="Leave project context"
              onClick={() => setActiveProject(null)}
              className="inline-flex shrink-0 items-center justify-center rounded-full p-0.5 hover:bg-card hover:text-foreground"
            >
              <X size={13} />
            </button>
          )}
        </div>
      )}
      <div
        id="composer"
        className="mx-auto flex max-w-[52rem] items-end gap-[0.35rem] rounded-[1.6rem] border border-border-strong bg-background py-[0.35rem] pl-4 pr-[0.4rem] shadow-sm focus-within:border-primary focus-within:ring-[3px] focus-within:ring-primary/20"
      >
        <textarea
          id="prompt-input"
          ref={composerRef}
          rows={1}
          placeholder="Message the agent..."
          disabled={!enabled}
          onInput={(e) => autoGrow(e.currentTarget)}
          onKeyDown={onKeyDown}
          className="max-h-[40vh] min-h-[2.2rem] flex-1 resize-none overflow-y-auto bg-transparent py-[0.44rem] text-[0.95rem] leading-[1.4] text-foreground outline-none placeholder:text-muted-foreground disabled:opacity-60"
        />
        <div id="composer-actions" className="flex items-center gap-1">
          <button
            aria-label="Voice input"
            title={voice.title}
            disabled={voice.disabled}
            onClick={voice.onClick}
            className={cn(
              ROUND,
              "disabled:cursor-not-allowed disabled:opacity-35",
              voice.recording
                ? "mic-recording bg-destructive text-white"
                : "text-muted-foreground hover:bg-secondary hover:text-foreground"
            )}
          >
            <Mic size={18} />
          </button>
          {running ? (
            <button
              aria-label="Stop"
              title="Stop"
              onClick={cancel}
              className={cn(ROUND, "bg-destructive text-white hover:bg-danger-hover")}
            >
              <Square size={16} fill="currentColor" strokeWidth={0} />
            </button>
          ) : (
            <button
              aria-label="Send"
              title="Send"
              disabled={!enabled}
              onClick={send}
              className={cn(
                ROUND,
                "bg-primary text-primary-foreground hover:bg-primary-hover disabled:cursor-not-allowed disabled:opacity-35"
              )}
            >
              <ArrowUp size={18} />
            </button>
          )}
        </div>
      </div>
      <SnippetBar />
    </div>
  );
}
