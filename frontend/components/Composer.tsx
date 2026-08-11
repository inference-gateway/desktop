import { ArrowUp, Folder, Mic, Paperclip, Square, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { StatusBar } from "./StatusBar";
import { TokenReadout } from "./TokenReadout";
import { SnippetBar } from "./SnippetBar";
import { useVoiceInput } from "@/hooks/useVoiceInput";
import { autoGrow } from "@/lib/textarea";
import { api } from "@/lib/tauri";
import { useRef, useState } from "react";

const ROUND = "inline-flex h-[2.2rem] w-[2.2rem] items-center justify-center rounded-full";
const ALLOWED = ["image/png", "image/jpeg", "image/svg+xml", "application/pdf"];

type PendingImage = { id: string; dataUrl: string; file: File };

export function Composer() {
  const { composerRef, enabled, running, send, cancel, setStatus, setError, history, activeProject, setActiveProject, currentProject } = useDesktop();
  const voice = useVoiceInput({ textareaRef: composerRef, running, setStatus, setError });
  const cursorRef = useRef(-1);
  const draftRef = useRef("");
  const fileRef = useRef<HTMLInputElement>(null);
  const [pending, setPending] = useState<PendingImage[]>([]);

  const addImage = (file: File) => {
    if (!ALLOWED.includes(file.type)) {
      setError(`Unsupported file type: ${file.type}`);
      return;
    }
    const reader = new FileReader();
    reader.onload = (ev) => {
      const dataUrl = ev.target?.result as string;
      setPending((prev) => [...prev, { id: crypto.randomUUID(), dataUrl, file }]);
    };
    reader.readAsDataURL(file);
  };

  const onPaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const file = e.clipboardData.files?.[0];
    if (!file) return;
    e.preventDefault();
    addImage(file);
  };

  const onFilePick = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) addImage(file);
    e.target.value = "";
  };

  const onSend = async () => {
    if (pending.length > 0) {
      const el = composerRef.current;
      if (!el) return;
      const text = el.value.trim();
      const paths: string[] = [];
      for (const img of pending) {
        try {
          const base64 = img.dataUrl.split(",")[1];
          const saved = await api.saveUpload(base64, img.file.type);
          paths.push(saved);
        } catch (e) {
          setError(`Failed to save image: ${e}`);
          return;
        }
      }
      const refs = paths.map((p) => `[Attached image: ${p}]`).join("\n");
      el.value = text ? `${refs}\n\n${text}` : refs;
      autoGrow(el);
      setPending([]);
    }
    send();
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget;
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      cursorRef.current = -1;
      draftRef.current = "";
      onSend();
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
        className="mx-auto flex max-w-[52rem] flex-col rounded-[1.6rem] border border-border-strong bg-background shadow-sm focus-within:border-primary focus-within:ring-[3px] focus-within:ring-primary/20"
      >
        {pending.length > 0 && (
          <div className="flex flex-wrap gap-2 border-b border-border px-3 pt-2 pb-2">
            {pending.map((img) => (
              <div key={img.id} className="group relative inline-block h-14 w-14 shrink-0 overflow-hidden rounded-md border border-border">
                <img src={img.dataUrl} alt="" className="h-full w-full object-cover" />
                <button
                  aria-label="Remove image"
                  onClick={() => setPending((prev) => prev.filter((p) => p.id !== img.id))}
                  className="absolute right-0.5 top-0.5 flex h-4 w-4 items-center justify-center rounded-full bg-black/60 text-white opacity-0 transition-opacity group-hover:opacity-100"
                >
                  <X size={10} />
                </button>
              </div>
            ))}
          </div>
        )}
        <div className="flex items-end gap-[0.35rem] py-[0.35rem] pl-4 pr-[0.4rem]">
          <textarea
            id="prompt-input"
            ref={composerRef}
            rows={1}
            placeholder="Message the agent..."
            disabled={!enabled}
            onPaste={onPaste}
            onInput={(e) => autoGrow(e.currentTarget)}
            onKeyDown={onKeyDown}
            className="max-h-[40vh] min-h-[2.2rem] flex-1 resize-none overflow-y-auto bg-transparent py-[0.44rem] text-[0.95rem] leading-[1.4] text-foreground outline-none placeholder:text-muted-foreground disabled:opacity-60"
          />
          <div id="composer-actions" className="flex items-center gap-1">
            <input
              ref={fileRef}
              type="file"
              accept="image/png,image/jpeg,image/svg+xml,application/pdf"
              className="hidden"
              onChange={onFilePick}
            />
            <button
              aria-label="Attach file"
              title="Attach image or file"
              disabled={!enabled}
              onClick={() => fileRef.current?.click()}
              className={cn(
                ROUND,
                "text-muted-foreground hover:bg-secondary hover:text-foreground disabled:cursor-not-allowed disabled:opacity-35",
              )}
            >
              <Paperclip size={16} />
            </button>
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
                  : "text-muted-foreground hover:bg-secondary hover:text-foreground",
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
                onClick={onSend}
                className={cn(
                  ROUND,
                  "bg-primary text-primary-foreground hover:bg-primary-hover disabled:cursor-not-allowed disabled:opacity-35",
                )}
              >
                <ArrowUp size={18} />
              </button>
            )}
          </div>
        </div>
      </div>
      <SnippetBar />
    </div>
  );
}
