import { useEffect, useState, type KeyboardEvent } from "react";
import { ImagePlus, X } from "lucide-react";
import { useDesktop } from "@/store";
import { api, type MarkdownAgent } from "@/lib/tauri";
import { normaliseAgentName } from "@/lib/agents";
import { AgentAvatar } from "./AgentAvatar";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

const QUICK_EMOJIS = ["🤖", "🧠", "🛠️", "🔍", "✍️", "🎨", "📊", "🧪", "🚀", "🦉"];

const EMPTY: MarkdownAgent = {
  name: "",
  description: "",
  emoji: "",
  model: "",
  instructions: "",
  avatar: null,
  path: "",
};

// Creating or editing an agent takes over the chat column, Grok Bot style:
// profile on top, instructions below, and Save drops you into a chat with it.
export function AgentEditor() {
  const { agentEditor, closeAgentEditor, saveAgent, models, setError } = useDesktop();
  const existing = agentEditor?.agent ?? null;
  const [draft, setDraft] = useState<MarkdownAgent>(existing ?? EMPTY);
  const [agentsDir, setAgentsDir] = useState("~/.infer/agents");
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    api
      .getConfig()
      .then((cfg) => setAgentsDir(cfg.agents_dir || "~/.infer/agents"))
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

  const choosePicture = async () => {
    try {
      if (await api.pickAgentAvatar(draft.name)) await refreshAvatar();
    } catch (e) {
      setError(String(e));
    }
  };

  const removePicture = async () => {
    try {
      await api.removeAgentAvatar(draft.name);
      set("avatar", null);
    } catch (e) {
      setError(String(e));
    }
  };

  const save = async () => {
    if (!complete || saving) return;
    setSaving(true);
    try {
      await saveAgent(draft, existing?.name);
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
            <div className="flex flex-wrap gap-1">
              {QUICK_EMOJIS.map((e) => (
                <button
                  key={e}
                  type="button"
                  aria-label={`Use emoji ${e}`}
                  aria-pressed={draft.emoji === e}
                  onClick={() => set("emoji", draft.emoji === e ? "" : e)}
                  className={`rounded-md px-1.5 py-0.5 text-[1.1rem] hover:bg-secondary ${draft.emoji === e ? "ring-2 ring-primary" : ""}`}
                >
                  {e}
                </button>
              ))}
              <Input
                id="agent-emoji"
                aria-label="Agent emoji"
                value={draft.emoji}
                placeholder="emoji"
                onChange={(e) => set("emoji", e.target.value)}
                className="w-20"
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
            {agentsDir}/{draft.name || "<name>"}.md
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
            {models.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        </div>

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
