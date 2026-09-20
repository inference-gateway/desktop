import { ArrowUp, Folder, Mic, Plus, Square, Terminal, Wrench, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { StatusBar } from "./StatusBar";
import { SnippetBar } from "./SnippetBar";
import { useVoiceInput } from "@/hooks/useVoiceInput";
import { autoGrow } from "@/lib/textarea";
import { isBashCommand } from "@/lib/tools";
import { fetchSkillsCatalog, type SkillMetadata } from "@/lib/skills";
import { api } from "@/lib/tauri";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

type SlashItem = { name: string; description: string; kind: "command" | "skill" };

const ROUND = "inline-flex h-[2.2rem] w-[2.2rem] items-center justify-center rounded-full";

type PendingFile = { id: string; path: string; preview?: string };

/// Labels a file by extension for the chip preview and the prompt marker wording
/// (`[Attached image|video|audio|file: <path>]`). Acceptance itself is owned by
/// the backend (`projects_allowed_mimes`) - this only labels.
const kindOf = (name: string) => {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (["mp4", "mov", "webm"].includes(ext)) return "video" as const;
  if (["mp3", "wav", "m4a", "aac"].includes(ext)) return "audio" as const;
  if (["png", "jpg", "jpeg", "heic", "heif", "gif", "webp", "svg"].includes(ext)) return "image" as const;
  return "file" as const;
};

export function Composer() {
  const {
    composerRef,
    ready,
    enabled,
    running,
    send,
    cancel,
    queuedPrompt,
    discardQueued,
    popQueued,
    setStatus,
    setError,
    history,
    bashHistory,
    activeProject,
    setActiveProject,
    currentProject,
    initSelecting,
    initSelection,
    cancelInitSelection,
    tools,
    shortcuts,
  } = useDesktop();
  const selCount = initSelection.size;
  const broadcasting = initSelecting && selCount > 0;
  const voice = useVoiceInput({ textareaRef: composerRef, running, setStatus, setError });
  const cursorRef = useRef(-1);
  const draftRef = useRef("");
  const [pending, setPending] = useState<PendingFile[]>([]);
  const [dragOver, setDragOver] = useState(false);
  const [skills, setSkills] = useState<SkillMetadata[]>([]);
  const [installedSkills, setInstalledSkills] = useState<Set<string>>(new Set());
  const [installing, setInstalling] = useState(false);
  const [installError, setInstallError] = useState("");
  const [showSkills, setShowSkills] = useState(false);
  const [skillQuery, setSkillQuery] = useState("");
  const [activeSkillIdx, setActiveSkillIdx] = useState(0);
  const [pendingDownload, setPendingDownload] = useState<SlashItem | null>(null);
  const skillsRef = useRef<HTMLDivElement>(null);
  const [toolQuery, setToolQuery] = useState("");
  const [showTools, setShowTools] = useState(false);
  const [activeToolIdx, setActiveToolIdx] = useState(0);
  const [bashMode, setBashMode] = useState(false);
  const [toolMode, setToolMode] = useState(false);
  const toolsRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    for (const list of [toolsRef.current, skillsRef.current]) {
      list?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
    }
  }, [activeToolIdx, activeSkillIdx]);

  const loadSkills = useCallback(async () => {
    try {
      const cat = await fetchSkillsCatalog();
      setSkills(cat.skills);
      setInstalledSkills(new Set(await api.listInstalledSkills()));
    } catch {}
  }, []);

  useEffect(() => {
    if (skills.length === 0) loadSkills();
  }, [loadSkills]);

  useEffect(() => {
    if (!showSkills) return;
    api
      .listInstalledSkills()
      .then((names) => setInstalledSkills(new Set(names)))
      .catch(() => {});
  }, [showSkills]);

  /// Drop, paste and picker all land here: the backend saves the file at attach
  /// time (media pool or uploads, and validates the allowlist), so a rejection
  /// surfaces immediately instead of at send time.
  const addFile = async (file: File) => {
    try {
      const path = await api.attachBytes(currentProject, file.name, await file.arrayBuffer());
      setPending((prev) => [
        ...prev,
        {
          id: crypto.randomUUID(),
          path,
          preview: kindOf(file.name) === "image" ? URL.createObjectURL(file) : undefined,
        },
      ]);
    } catch (e) {
      setError(`Failed to attach: ${e}`);
    }
  };

  const onPaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const file = e.clipboardData.files?.[0];
    if (!file) return;
    e.preventDefault();
    addFile(file);
  };

  const onAttachPick = async () => {
    try {
      const path = await api.attachPick(currentProject);
      if (path) setPending((prev) => [...prev, { id: crypto.randomUUID(), path }]);
    } catch (e) {
      setError(`Failed to attach: ${e}`);
    }
  };

  const handleInput = (e: React.FormEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget;
    if ((e.nativeEvent as InputEvent).inputType === "insertText") {
      if (el.value === "!") el.value = "! ";
      else if (el.value === "! !") el.value = "!! ";
    }
    autoGrow(el);
    setToolMode(el.value.startsWith("!!"));
    if (isBashCommand(el.value)) {
      setShowSkills(false);
      setShowTools(false);
      setBashMode(true);
      return;
    }
    setBashMode(false);
    if (el.value.startsWith("!!")) {
      setShowSkills(false);
      if (!el.value.slice(2).includes("(")) {
        setToolQuery(el.value.slice(2).trimStart().toLowerCase());
        setShowTools(true);
        setActiveToolIdx(0);
      } else {
        setShowTools(false);
      }
      return;
    }
    const pos = el.selectionStart;
    const text = el.value;
    let i = pos - 1;
    while (i >= 0 && text[i] !== "/" && text[i] !== " " && text[i] !== "\n") i--;
    if (i >= 0 && text[i] === "/") {
      const query = text.slice(i + 1, pos).toLowerCase();
      setSkillQuery(query);
      setShowSkills(true);
      setActiveSkillIdx(0);
    } else {
      setShowSkills(false);
    }
  };

  const selectSkill = (item: SlashItem) => {
    const el = composerRef.current;
    if (!el) return;
    const pos = el.selectionStart;
    const text = el.value;
    let i = pos - 1;
    while (i >= 0 && text[i] !== "/") i--;
    el.value = text.slice(0, i) + "/" + item.name + " ";
    autoGrow(el);
    setShowSkills(false);
    if (item.kind === "skill" && !installedSkills.has(item.name)) {
      setInstallError("");
      setPendingDownload(item);
    }
  };

  const selectTool = (tool: string) => {
    const el = composerRef.current;
    if (!el) return;
    el.value = "!! " + tool + "(";
    autoGrow(el);
    setShowTools(false);
  };

  const localOnlySkills = Array.from(installedSkills)
    .filter((name) => !skills.some((s) => s.name === name))
    .sort()
    .map((name) => ({ name, description: "" }));
  const slashItems: SlashItem[] = [
    ...shortcuts.map((c) => ({ ...c, kind: "command" as const })),
    ...[...skills, ...localOnlySkills].map((s) => ({
      name: s.name,
      description: s.description,
      kind: "skill" as const,
    })),
  ];
  const filteredSkills = showSkills
    ? slashItems.filter(
        (s) => s.name.toLowerCase().includes(skillQuery) || s.description.toLowerCase().includes(skillQuery),
      )
    : [];

  const filteredTools = showTools ? tools.filter((t) => t.toLowerCase().includes(toolQuery)) : [];

  const onSend = () => {
    const composer = composerRef.current;
    if (composer && composer.value.trimStart().startsWith("!!")) {
      composer.value = composer.value.replace(/[\u201c\u201d]/g, '"');
    }
    if (composer) composer.value = composer.value.replace(/^(!!?) +/, "$1");
    if (pending.length > 0) {
      const el = composerRef.current;
      if (!el) return;
      const text = el.value.trim();
      const refs = pending.map((item) => `[Attached ${kindOf(item.path)}: ${item.path}]`).join("\n");
      el.value = text ? `${refs}\n\n${text}` : refs;
      autoGrow(el);
      setPending([]);
    }
    send();
    setBashMode(false);
    setToolMode(false);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget;
    if (showTools && filteredTools.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setActiveToolIdx((prev) => Math.min(prev + 1, filteredTools.length - 1));
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setActiveToolIdx((prev) => Math.max(prev - 1, 0));
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        selectTool(filteredTools[activeToolIdx]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setShowTools(false);
        return;
      }
    }
    if (showSkills && filteredSkills.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setActiveSkillIdx((prev) => Math.min(prev + 1, filteredSkills.length - 1));
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setActiveSkillIdx((prev) => Math.max(prev - 1, 0));
        return;
      }
      if (e.key === "Enter") {
        e.preventDefault();
        selectSkill(filteredSkills[activeSkillIdx]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setShowSkills(false);
        return;
      }
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      cursorRef.current = -1;
      draftRef.current = "";
      onSend();
      return;
    }
    if (e.key === "ArrowUp") {
      const queued = popQueued();
      if (queued != null) {
        e.preventDefault();
        cursorRef.current = -1;
        draftRef.current = "";
        el.value = queued;
        autoGrow(el);
        return;
      }
    }
    const recall = bashMode ? bashHistory : history;
    if (e.key === "ArrowUp" && recall.length > 0) {
      e.preventDefault();
      if (cursorRef.current === -1) {
        draftRef.current = el.value;
        cursorRef.current = recall.length - 1;
      } else if (cursorRef.current > 0) {
        cursorRef.current--;
      } else {
        return;
      }
      el.value = recall[cursorRef.current];
      autoGrow(el);
      return;
    }
    if (e.key === "ArrowDown" && recall.length > 0 && cursorRef.current !== -1) {
      e.preventDefault();
      if (cursorRef.current < recall.length - 1) {
        cursorRef.current++;
        el.value = recall[cursorRef.current];
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
        onDragOver={(e) => {
          e.preventDefault();
          setDragOver(true);
        }}
        onDragLeave={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragOver(false);
        }}
        onDrop={(e) => {
          e.preventDefault();
          setDragOver(false);
          for (const file of Array.from(e.dataTransfer.files)) addFile(file);
        }}
        className={cn(
          "relative mx-auto flex max-w-[52rem] flex-col rounded-[1.6rem] border border-border-strong bg-background shadow-sm transition-colors focus-within:border-primary focus-within:ring-[3px] focus-within:ring-primary/20",
          bashMode && "border-tool focus-within:border-tool focus-within:ring-tool/20",
          toolMode && "border-warn focus-within:border-warn focus-within:ring-warn/20",
          dragOver && "border-dashed border-primary",
        )}
      >
        {dragOver && (
          <div className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-[1.6rem] border-2 border-dashed border-primary bg-background/80 text-sm font-medium text-primary">
            Drop to attach
          </div>
        )}
        {queuedPrompt && (
          <div className="flex items-center gap-2 border-b border-border px-4 py-1.5 text-[0.8rem] text-muted-foreground">
            <span className="min-w-0 flex-1 truncate">Queued: {queuedPrompt}</span>
            <button
              aria-label="Discard queued prompt"
              title="Discard queued prompt"
              onClick={discardQueued}
              className="shrink-0 rounded p-0.5 hover:bg-secondary hover:text-foreground"
            >
              <X size={13} />
            </button>
          </div>
        )}
        {bashMode && (
          <div className="flex items-center gap-2 border-b border-tool/30 px-4 py-1.5 text-[0.8rem] text-muted-foreground">
            <Terminal size={13} className="shrink-0 text-tool" />
            <span>
              <span className="font-medium text-tool">bash mode</span> - Enter runs the command in the workspace, output
              lands in the conversation
            </span>
          </div>
        )}
        {toolMode && (
          <div className="flex items-center gap-2 border-b border-warn/30 px-4 py-1.5 text-[0.8rem] text-muted-foreground">
            <Wrench size={13} className="shrink-0 text-warn" />
            <span>
              <span className="font-medium text-warn">tool mode</span> - Enter runs the tool call directly
            </span>
          </div>
        )}
        {pending.length > 0 && (
          <div className="flex flex-wrap gap-2 border-b border-border px-3 pt-2 pb-2">
            {pending.map((item) => (
              <div
                key={item.id}
                className="group relative inline-block h-14 w-14 shrink-0 overflow-hidden rounded-md border border-border bg-secondary"
              >
                {item.preview ? (
                  <img src={item.preview} alt="" className="h-full w-full object-cover" />
                ) : (
                  <span className="flex h-full w-full items-center justify-center px-1 text-center text-[0.6rem] font-medium uppercase text-muted-foreground">
                    {kindOf(item.path)}
                  </span>
                )}
                <button
                  aria-label="Remove attachment"
                  onClick={() => setPending((prev) => prev.filter((p) => p.id !== item.id))}
                  className="absolute right-0.5 top-0.5 flex h-4 w-4 items-center justify-center rounded-full bg-black/60 text-white opacity-0 transition-opacity group-hover:opacity-100"
                >
                  <X size={10} />
                </button>
              </div>
            ))}
          </div>
        )}
        {filteredTools.length > 0 && (
          <div
            ref={toolsRef}
            className="mx-2 mb-2 max-h-[40vh] overflow-y-auto rounded-lg border border-border bg-popover p-1 shadow-lg"
          >
            {filteredTools.map((t, i) => (
              <button
                key={t}
                aria-selected={i === activeToolIdx}
                onClick={() => selectTool(t)}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[0.82rem]",
                  i === activeToolIdx
                    ? "bg-primary/15 text-primary"
                    : "text-muted-foreground hover:bg-secondary hover:text-foreground",
                )}
              >
                <span className="min-w-0 flex-1 truncate font-medium">{t}</span>
              </button>
            ))}
          </div>
        )}
        {filteredSkills.length > 0 && (
          <div
            ref={skillsRef}
            className="mx-2 mb-2 max-h-[40vh] overflow-y-auto rounded-lg border border-border bg-popover p-1 shadow-lg"
          >
            {filteredSkills.map((s, i) => {
              const isConfigured = s.kind === "command" || installedSkills.has(s.name);
              const label = s.kind === "command" ? "command" : isConfigured ? "skill" : "remote skill";
              return (
                <button
                  key={s.name}
                  aria-selected={i === activeSkillIdx}
                  onClick={() => selectSkill(s)}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[0.82rem]",
                    i === activeSkillIdx
                      ? isConfigured
                        ? "bg-primary/15 text-primary"
                        : "bg-secondary text-foreground"
                      : isConfigured
                        ? "text-primary"
                        : "text-muted-foreground hover:bg-secondary hover:text-foreground",
                  )}
                >
                  <span className="shrink-0 font-medium">{s.name}</span>
                  <span className="min-w-0 flex-1 truncate text-muted-foreground">{s.description}</span>
                  <span className="shrink-0 rounded bg-secondary px-1.5 py-0.5 text-[0.65rem] text-muted-foreground">
                    {label}
                  </span>
                </button>
              );
            })}
          </div>
        )}
        {pendingDownload && (
          <Dialog open onOpenChange={() => !installing && setPendingDownload(null)}>
            <DialogContent>
              <DialogHeader>
                <DialogTitle>Download skill</DialogTitle>
                <DialogDescription>
                  The skill <strong>{pendingDownload.name}</strong> is not yet downloaded locally. Would you like to
                  download it now?
                </DialogDescription>
              </DialogHeader>
              {installError && <p className="text-[0.8rem] text-err">{installError}</p>}
              <DialogFooter>
                <Button variant="outline" disabled={installing} onClick={() => setPendingDownload(null)}>
                  Deny
                </Button>
                <Button
                  disabled={installing}
                  onClick={async () => {
                    setInstalling(true);
                    setInstallError("");
                    try {
                      await api.installSkill(pendingDownload.name);
                      setInstalledSkills(new Set(await api.listInstalledSkills()));
                      setPendingDownload(null);
                    } catch (e) {
                      setInstallError(String(e));
                    } finally {
                      setInstalling(false);
                    }
                  }}
                >
                  {installing ? "Installing..." : "Approve"}
                </Button>
              </DialogFooter>
            </DialogContent>
          </Dialog>
        )}
        {initSelecting && (
          <div className="flex items-center justify-between gap-2 border-b border-border px-4 py-1.5 text-[0.8rem] text-muted-foreground">
            <span>
              {selCount === 0
                ? "Select projects to broadcast to"
                : `Broadcasting to ${selCount} project${selCount === 1 ? "" : "s"}`}
            </span>
            <button
              onClick={cancelInitSelection}
              aria-label="Exit multi-project mode"
              className="rounded px-1.5 py-0.5 hover:bg-secondary hover:text-foreground"
            >
              Exit
            </button>
          </div>
        )}
        <div className="flex items-end gap-[0.35rem] px-[0.4rem] py-[0.35rem]">
          <button
            aria-label="Attach file"
            title="Attach image or file"
            disabled={!enabled}
            onClick={onAttachPick}
            className={cn(
              ROUND,
              "text-muted-foreground hover:bg-secondary hover:text-foreground disabled:cursor-not-allowed disabled:opacity-35",
            )}
          >
            <Plus size={16} />
          </button>
          <textarea
            id="prompt-input"
            ref={composerRef}
            rows={1}
            placeholder={
              broadcasting
                ? `Message ${selCount} selected project${selCount === 1 ? "" : "s"}...`
                : running
                  ? "Write a follow-up..."
                  : "Message the orchestrator..."
            }
            disabled={!ready}
            onPaste={onPaste}
            onInput={handleInput}
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
                  : "text-muted-foreground hover:bg-secondary hover:text-foreground",
              )}
            >
              <Mic size={18} />
            </button>
            {running && !initSelecting ? (
              <button
                aria-label="Stop"
                title="Stop (Esc)"
                onClick={cancel}
                className={cn(ROUND, "bg-destructive text-white hover:bg-danger-hover")}
              >
                <Square size={16} fill="currentColor" strokeWidth={0} />
              </button>
            ) : (
              <button
                aria-label="Send"
                title="Send"
                disabled={initSelecting ? !ready || selCount === 0 : !enabled}
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
