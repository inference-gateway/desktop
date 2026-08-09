import { ArrowUp, Mic, Square } from "lucide-react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { useVoiceInput } from "@/hooks/useVoiceInput";
import { autoGrow } from "@/lib/textarea";

const ROUND = "inline-flex h-[2.2rem] w-[2.2rem] items-center justify-center rounded-full";

export function Composer() {
  const { composerRef, enabled, running, send, cancel, setStatus, setError } = useDesktop();
  const voice = useVoiceInput({ textareaRef: composerRef, running, setStatus, setError });

  return (
    <div id="input-area" className="border-t border-border bg-card px-4 pb-4 pt-[0.6rem]">
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
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send();
            }
          }}
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
    </div>
  );
}
