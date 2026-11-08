import { useEffect, useState, type KeyboardEvent } from "react";
import { ImagePlus, KeyRound, Mail, X } from "lucide-react";
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
  project: "",
  email: "",
  allowed_recipients: [],
  mcp_servers: [],
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
  const { agentEditor, closeAgentEditor, saveAgent, models, projectNames, setError } = useDesktop();
  const existing = agentEditor?.agent ?? null;
  const [draft, setDraft] = useState<MarkdownAgent>(existing ?? EMPTY);
  const [emailKey, setEmailKey] = useState("");
  const [recipients, setRecipients] = useState((existing?.allowed_recipients ?? []).join(", "));
  const [creating, setCreating] = useState(false);
  const [savingInbox, setSavingInbox] = useState(false);
  const [inboxNotice, setInboxNotice] = useState("");
  const [saving, setSaving] = useState(false);
  const [animatedPick, setAnimatedPick] = useState<FluentEmoji | null>(null);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [mcpServers, setMcpServers] = useState<string[]>([]);
  useEffect(() => {
    api
      .mcpSettings()
      .then((settings) => setMcpServers(settings.servers.map((server) => server.name)))
      .catch(() => {});
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

  const createInbox = async () => {
    const key = emailKey.trim();
    if (!draft.name || !key || creating) return;
    setCreating(true);
    try {
      set("email", (await api.createAgentInbox(draft.name, key)).trim());
    } catch (e) {
      setError(String(e));
    } finally {
      setCreating(false);
    }
  };

  const saveInboxSecret = async () => {
    if (!existing?.email.trim() || savingInbox) return;
    setSavingInbox(true);
    setInboxNotice("");
    try {
      const saved = await api.saveInboxSecret(existing.name, emailKey.trim());
      setInboxNotice(
        saved.workspaces.length > 0
          ? `Saved as the secret ${saved.label} for workspace ${saved.workspaces.join(", ")}. Restart the CLI so the daemon loads it.`
          : `Saved as the secret ${saved.label}. Assign it to the agent's computer under Settings > Secrets.`,
      );
      if (saved.failures.length > 0) setError(`Not every cluster took the inbox key: ${saved.failures.join("; ")}`);
    } catch (e) {
      setError(String(e));
    } finally {
      setSavingInbox(false);
    }
  };

  const save = async () => {
    if (!complete || saving) return;
    setSaving(true);
    try {
      const agent = {
        ...draft,
        allowed_recipients: recipients
          .split(",")
          .map((r) => r.trim())
          .filter(Boolean),
      };
      await saveAgent(agent, existing?.name, animatedPick ? fluentAnimatedUrl(animatedPick) : undefined);
      if (existing?.name && existing.name !== agent.name) {
        await api.moveAgentEmailKey(existing.name, agent.name);
      }
      const key = emailKey.trim();
      if (key) await api.setAgentEmailKey(agent.name, key);
      if (agent.email.trim()) {
        const dropped = await api.syncAgentMail(agent.name);
        if (dropped.length > 0) {
          setError(`Not allowed by agent_mail.allowed_recipients in config.yaml: ${dropped.join(", ")}`);
        }
      }
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

        {(projectNames.length > 0 || draft.project) && (
          <div className="flex flex-col gap-1">
            <Label htmlFor="agent-project" className="text-[0.8rem] text-muted-foreground">
              Project
            </Label>
            <select
              id="agent-project"
              value={draft.project}
              onChange={(e) => set("project", e.target.value)}
              className="h-8 rounded-lg border border-input bg-transparent px-2.5 text-[0.85rem] text-foreground outline-none focus-visible:border-ring"
            >
              <option value="">No project</option>
              {draft.project && !projectNames.includes(draft.project) && (
                <option value={draft.project}>{draft.project} (missing)</option>
              )}
              {projectNames.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
            <span className="text-[0.72rem] text-muted-foreground/70">
              The project new chats with this agent open in. Pick another above the composer.
            </span>
          </div>
        )}

        <fieldset className="flex flex-col gap-1">
          <legend className="mb-1 text-[0.8rem] text-muted-foreground">MCP servers</legend>
          {mcpServers.length === 0 && draft.mcp_servers.length === 0 ? (
            <span className="text-[0.72rem] text-muted-foreground/70">
              No MCP server yet. Add one under Settings &gt; MCPs.
            </span>
          ) : (
            <div className="flex flex-wrap gap-x-4 gap-y-1.5">
              {[...new Set([...mcpServers, ...draft.mcp_servers])].map((name) => (
                <label key={name} className="flex items-center gap-2 text-[0.82rem]">
                  <input
                    type="checkbox"
                    id={`agent-mcp-${name}`}
                    checked={draft.mcp_servers.includes(name)}
                    onChange={(e) =>
                      set(
                        "mcp_servers",
                        e.target.checked ? [...draft.mcp_servers, name] : draft.mcp_servers.filter((s) => s !== name),
                      )
                    }
                    className="h-4 w-4 accent-primary"
                  />
                  {mcpServers.includes(name) ? name : `${name} (missing)`}
                </label>
              ))}
            </div>
          )}
          <span className="text-[0.72rem] text-muted-foreground/70">
            With none picked, the agent uses every server of its computer. A pick is honoured by an infer CLI that
            supports mcpServers.
          </span>
        </fieldset>

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

        <div className="flex flex-col gap-1">
          <Label htmlFor="agent-email" className="text-[0.8rem] text-muted-foreground">
            Email
          </Label>
          <div className="flex gap-2">
            <Input
              id="agent-email"
              value={draft.email}
              placeholder="code-reviewer@agentmail.to"
              onKeyDown={advanceOnEnter("agent-email-key")}
              onChange={(e) => set("email", e.target.value)}
            />
            <Button
              size="sm"
              variant="outline"
              disabled={!draft.name || !emailKey.trim() || creating}
              title={emailKey.trim() ? "Provision the inbox through AgentMail" : "Type the AgentMail API key first"}
              onClick={createInbox}
            >
              <Mail />
              {creating ? "Creating..." : "Create inbox"}
            </Button>
          </div>
          <div className="flex flex-col gap-1">
            <Label htmlFor="agent-email-key" className="text-[0.8rem] text-muted-foreground">
              AgentMail API key
            </Label>
            <Input
              id="agent-email-key"
              type="password"
              autoComplete="off"
              value={emailKey}
              placeholder="am_us_..."
              onKeyDown={advanceOnEnter("agent-email-recipients")}
              onChange={(e) => setEmailKey(e.target.value)}
            />
          </div>
          <div className="flex items-center gap-2">
            <Button
              size="sm"
              variant="outline"
              disabled={!existing?.email.trim() || savingInbox}
              title={
                existing?.email.trim()
                  ? "Mint a key that can only use this inbox and keep it as AGENTMAIL_API_KEY for the agent's computer"
                  : "Save the agent with its inbox address first"
              }
              onClick={saveInboxSecret}
            >
              <KeyRound />
              {savingInbox ? "Saving..." : "Save inbox key as secret"}
            </Button>
            {inboxNotice && (
              <span aria-live="polite" className="text-[0.72rem] text-muted-foreground">
                {inboxNotice}
              </span>
            )}
          </div>
          <div className="flex flex-col gap-1">
            <Label htmlFor="agent-email-recipients" className="text-[0.8rem] text-muted-foreground">
              Allowed recipients
            </Label>
            <Input
              id="agent-email-recipients"
              value={recipients}
              placeholder="you@example.com, other-agent@agentmail.to"
              onChange={(e) => setRecipients(e.target.value)}
            />
          </div>
          <span className="text-[0.72rem] text-muted-foreground/70">
            Comma-separated addresses or domains the agent may mail - with none it can only mail itself, and
            agent_mail.allowed_recipients in config.yaml caps every agent. The address travels with the agent's file,
            the API key stays in the credential store and never in the file. The inbox key saved as a secret can only
            use this inbox, so the agent's computer never holds the API key.
          </span>
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
