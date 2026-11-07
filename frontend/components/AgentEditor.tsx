import { useEffect, useState, type KeyboardEvent } from "react";
import { ImagePlus, X } from "lucide-react";
import { useDesktop } from "@/store";
import { api, type MarkdownAgent, type Workspace } from "@/lib/tauri";
import { normaliseAgentName } from "@/lib/agents";
import { FLUENT_EMOJIS, fluentAnimatedUrl, fluentStaticUrl, type FluentEmoji } from "@/lib/fluent-emoji";
import { AgentAvatar } from "./AgentAvatar";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

const EMPTY: MarkdownAgent = {
  name: "",
  description: "",
  emoji: "",
  model: "",
  workspace: "",
  instructions: "",
  avatar: null,
  path: "",
};

// Creating or editing an agent takes over the chat column, Grok Bot style:
// profile on top, instructions below, and Save drops you into a chat with it.
/** The folder an agent's file is saved in: where it already lives, else
 * ~/.infer/agents for a new one. */
function agentFolder(agent: MarkdownAgent | null): string {
  return agent?.path ? agent.path.slice(0, agent.path.lastIndexOf("/")) : "~/.infer/agents";
}

export function AgentEditor() {
  const { agentEditor, closeAgentEditor, saveAgent, models, setError } = useDesktop();
  const existing = agentEditor?.agent ?? null;
  const [draft, setDraft] = useState<MarkdownAgent>(existing ?? EMPTY);
  const [saving, setSaving] = useState(false);
  const [animatedPick, setAnimatedPick] = useState<FluentEmoji | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  useEffect(() => {
    api
      .listWorkspaces()
      .then((all) => setWorkspaces(all.filter((w) => w.kind !== "local")))
      .catch(() => {});
  }, []);

  const set = <K extends keyof MarkdownAgent>(key: K, value: MarkdownAgent[K]) =>
    setDraft((d) => ({ ...d, [key]: value }));
  const advanceOnEnter = (nextId: string) => (e: KeyboardEvent) => {
    if (e.key !== "Enter") return;
    e.preventDefault();
    document.getElementById(nextId)?.focus();
  };
  const complete = draft.name.length > 0 && draft.description.trim().length > 0 && draft.instructions.trim().length > 0;

  const refreshAvatar = async () => {
    const fresh = (await api.listMarkdownAgents()).find((a) => a.name === draft.name);
    set("avatar", fresh?.avatar ?? null);
  };

  // A Fluent emoji previews its animated PNG straight from GitHub and is
  // downloaded beside the agent file on save, so it keeps moving offline.
  const pickAnimated = (pick: FluentEmoji) => {
    if (animatedPick?.folder === pick.folder) {
      setAnimatedPick(null);
      set("avatar", existing?.avatar ?? null);
      return;
    }
    setAnimatedPick(pick);
    setDraft((d) => ({ ...d, emoji: pick.emoji, avatar: fluentAnimatedUrl(pick) }));
  };

  const choosePicture = async () => {
    try {
      setAnimatedPick(null);
      if (await api.pickAgentAvatar(draft.name)) await refreshAvatar();
    } catch (e) {
      setError(String(e));
    }
  };

  const removePicture = async () => {
    try {
      setAnimatedPick(null);
      if (draft.name) await api.removeAgentAvatar(draft.name);
      set("avatar", null);
    } catch (e) {
      setError(String(e));
    }
  };

  const save = async () => {
    if (!complete || saving) return;
    setSaving(true);
    try {
      await saveAgent(draft, existing?.name, animatedPick ? fluentAnimatedUrl(animatedPick) : undefined);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div id="agent-editor" className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="mx-auto flex w-full max-w-[44rem] flex-col gap-5 px-6 py-8">
        <div className="flex items-start justify-between gap-4">
          <h2 className="text-[1.1rem] font-semibold">{existing ? `Edit ${existing.name}` : "New agent"}</h2>
          <button
            aria-label="Cancel agent editor"
            title="Cancel"
            onClick={closeAgentEditor}
            className="rounded-md p-1 text-muted-foreground hover:bg-secondary hover:text-foreground"
          >
            <X size={16} />
          </button>
        </div>

        <div className="flex items-center gap-4">
          <AgentAvatar agent={draft} size={72} className="text-[2.2rem]" />
          <div className="flex flex-col gap-2">
            <div className="flex max-w-[30rem] flex-wrap gap-1">
              {FLUENT_EMOJIS.map((e) => (
                <button
                  key={e.folder}
                  type="button"
                  aria-label={`Use animated ${e.label}`}
                  aria-pressed={animatedPick?.folder === e.folder}
                  title={`${e.label} (animates once picked)`}
                  onClick={() => pickAnimated(e)}
                  className={`rounded-md p-0.5 hover:bg-secondary ${animatedPick?.folder === e.folder ? "ring-2 ring-primary" : ""}`}
                >
                  <img src={fluentStaticUrl(e)} alt={e.emoji} width={32} height={32} loading="lazy" />
                </button>
              ))}
            </div>
            <div className="flex items-center gap-2 text-[0.75rem] text-muted-foreground">
              or type a plain emoji
              <Input
                id="agent-emoji"
                aria-label="Agent emoji"
                value={draft.emoji}
                placeholder="🤖"
                onChange={(e) => set("emoji", e.target.value)}
                className="w-16"
              />
            </div>
            <div className="flex gap-2">
              <Button
                size="sm"
                variant="outline"
                disabled={!draft.name}
                title={draft.name ? "Pick a GIF or picture" : "Name the agent first"}
                onClick={choosePicture}
              >
                <ImagePlus /> Choose picture
              </Button>
              {draft.avatar && (
                <Button size="sm" variant="ghost" onClick={removePicture}>
                  Remove picture
                </Button>
              )}
            </div>
          </div>
        </div>

        <div className="flex flex-col gap-1">
          <Label htmlFor="agent-name" className="text-[0.8rem] text-muted-foreground">
            Name
          </Label>
          <Input
            id="agent-name"
            value={draft.name}
            placeholder="code-reviewer"
            autoFocus={!existing}
            onKeyDown={advanceOnEnter("agent-description")}
            onChange={(e) => set("name", normaliseAgentName(e.target.value))}
          />
          <span className="text-[0.72rem] text-muted-foreground/70">
            {agentFolder(existing)}/{draft.name || "<name>"}.md
          </span>
        </div>

        <div className="flex flex-col gap-1">
          <Label htmlFor="agent-description" className="text-[0.8rem] text-muted-foreground">
            Description
          </Label>
          <Input
            id="agent-description"
            value={draft.description}
            placeholder="What it does and when another agent should hand work to it"
            onKeyDown={advanceOnEnter("agent-instructions")}
            onChange={(e) => set("description", e.target.value)}
          />
        </div>

        <div className="flex flex-col gap-1">
          <Label htmlFor="agent-model" className="text-[0.8rem] text-muted-foreground">
            Model
          </Label>
          <select
            id="agent-model"
            value={draft.model}
            onChange={(e) => set("model", e.target.value)}
            className="h-8 rounded-lg border border-input bg-transparent px-2.5 text-[0.85rem] text-foreground outline-none focus-visible:border-ring"
          >
            <option value="">Inherit the selected model</option>
            {draft.model && draft.model !== "inherit" && !models.includes(draft.model) && (
              <option value={draft.model}>{draft.model}</option>
            )}
            {models.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        </div>

        {(workspaces.length > 0 || draft.workspace) && (
          <div className="flex flex-col gap-1">
            <Label htmlFor="agent-workspace" className="text-[0.8rem] text-muted-foreground">
              Computer
            </Label>
            <select
              id="agent-workspace"
              value={draft.workspace}
              onChange={(e) => set("workspace", e.target.value)}
              className="h-8 rounded-lg border border-input bg-transparent px-2.5 text-[0.85rem] text-foreground outline-none focus-visible:border-ring"
            >
              <option value="">This computer</option>
              {draft.workspace && !workspaces.some((w) => w.name === draft.workspace) && (
                <option value={draft.workspace}>{draft.workspace} (not registered)</option>
              )}
              {workspaces.map((w) => (
                <option key={w.name} value={w.name} disabled={!w.reachable} title={w.error ?? undefined}>
                  {w.reachable ? w.name : `${w.name} (unreachable)`}
                </option>
              ))}
            </select>
            <span className="text-[0.72rem] text-muted-foreground/70">
              A workspace of the infer daemon. Each chat with this agent gets a computer of its own there.
            </span>
          </div>
        )}

        <div className="flex flex-col gap-1">
          <Label htmlFor="agent-instructions" className="text-[0.8rem] text-muted-foreground">
            Instructions
          </Label>
          <textarea
            id="agent-instructions"
            value={draft.instructions}
            rows={12}
            placeholder="You are a senior reviewer. Read the changed files and report findings ordered by severity."
            onChange={(e) => set("instructions", e.target.value)}
            className="min-h-[12rem] resize-y rounded-lg border border-input bg-transparent px-3 py-2 font-mono text-[0.82rem] leading-relaxed text-foreground outline-none focus-visible:border-ring"
          />
        </div>

        <div className="flex items-center justify-end gap-2">
          <Button variant="ghost" onClick={closeAgentEditor}>
            Cancel
          </Button>
          <Button disabled={!complete || saving} onClick={save}>
            Save agent
          </Button>
        </div>
      </div>
    </div>
  );
}
