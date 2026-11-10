import { useEffect, useState } from "react";
import { ArrowLeft, Eye, EyeOff } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ComputeTab } from "./ComputeTab";
import { SecretsTab } from "./SecretsTab";
import { McpTab } from "./McpTab";
import { WorkspacesTab } from "./WorkspacesTab";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { PROVIDERS, useDesktop } from "@/store";
import { useBackend } from "@/stores/backend";
import { useStatus } from "@/stores/status";
import { selectableVersions, stripV, versionAction } from "@/lib/updates";
import {} from "./GeneralTab";
import {} from "./GithubTab";
import {} from "./AgentsTab";
import {} from "./ProjectsTab";
import {} from "./SnippetsTab";
import {} from "./SystemPromptTab";
import {} from "./SkillsTab";
import {} from "./VoiceSamplesTab";
import {} from "./AvatarsTab";
import { GeneralTab } from "./GeneralTab";
import { GithubTab, GITHUB_SUBTABS, type GithubSubTab } from "./GithubTab";
import { AgentsTab } from "./AgentsTab";
import { ProjectsTab } from "./ProjectsTab";
import { SnippetsTab } from "./SnippetsTab";
import { SystemPromptTab } from "./SystemPromptTab";
import { SkillsTab } from "./SkillsTab";
import { VoiceSamplesTab } from "./VoiceSamplesTab";
import { AvatarsTab } from "./AvatarsTab";

type Tab =
  | "general"
  | "keys"
  | "secrets"
  | "mcps"
  | "prompt"
  | "updates"
  | "agents"
  | "compute"
  | "workspaces"
  | "snippets"
  | "projects"
  | "github"
  | "skills"
  | "voice"
  | "avatars";

const TABS: { id: Tab; label: string }[] = [
  { id: "general", label: "General" },
  { id: "keys", label: "API Keys" },
  { id: "secrets", label: "Secrets" },
  { id: "mcps", label: "MCPs" },
  { id: "prompt", label: "System Prompt" },
  { id: "agents", label: "Agents" },
  { id: "compute", label: "Compute" },
  { id: "workspaces", label: "Workspaces" },
  { id: "projects", label: "Projects" },
  { id: "snippets", label: "Snippets" },
  { id: "github", label: "GitHub" },
  { id: "skills", label: "Skills" },
  { id: "voice", label: "Voice samples" },
  { id: "avatars", label: "Avatars" },
  { id: "updates", label: "Updates" },
];

export function SettingsView() {
  const { updates, selectedVersions, selectVersion, installUpdate, checkForUpdates, applyUpdates, updateButton } =
    useBackend();
  const { setCurrentView, saveSettings, getAuth, initialSettingsTab, setInitialSettingsTab, setInitialProjectFilter } =
    useDesktop();
  const { statusText, statusError } = useStatus();
  const [tab, setTab] = useState<Tab>(
    TABS.some((t) => t.id === initialSettingsTab) ? (initialSettingsTab as Tab) : "general",
  );
  const [githubSub, setGithubSub] = useState<GithubSubTab>("repository");

  useEffect(() => {
    setInitialSettingsTab("general");
    setInitialProjectFilter("");
  }, [setInitialSettingsTab, setInitialProjectFilter]);
  const [values, setValues] = useState<Record<string, string>>({});
  const [shown, setShown] = useState<Record<string, boolean>>({});
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    getAuth()
      .then((auth) => {
        const next: Record<string, string> = {};
        for (const p of PROVIDERS) next[p.env] = (auth && auth[p.env]) || "";
        setValues(next);
      })
      .catch(() => {});
  }, [getAuth]);

  const onSave = () => {
    const keys: Record<string, string> = {};
    for (const p of PROVIDERS) keys[p.env] = (values[p.env] || "").trim();
    saveSettings(keys);
  };

  return (
    <div id="settings-view" className="flex min-h-0 flex-1">
      <nav className="flex w-[200px] shrink-0 flex-col gap-1 border-r border-border bg-secondary p-3">
        <button
          onClick={() => setCurrentView("chat")}
          className="mb-2 inline-flex items-center gap-1.5 rounded-md px-2 py-[0.45rem] text-[0.85rem] font-medium text-muted-foreground hover:bg-primary/10 hover:text-foreground"
        >
          <ArrowLeft size={15} /> Back
        </button>
        <span className="mb-1 px-2 text-[0.7rem] font-semibold uppercase tracking-wide text-muted-foreground">
          Settings
        </span>
        <div className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto">
          {TABS.map((t) => (
            <div key={t.id} className={cn("flex flex-col gap-1", t.id === "updates" && "mt-auto")}>
              <button
                aria-pressed={tab === t.id}
                onClick={() => setTab(t.id)}
                className={cn(
                  "relative rounded-md px-3 py-[0.5rem] text-left text-[0.85rem] font-medium",
                  tab === t.id
                    ? "bg-primary/15 text-primary before:absolute before:left-0 before:top-1/2 before:h-4 before:w-[3px] before:-translate-y-1/2 before:rounded-full before:bg-primary"
                    : "text-muted-foreground hover:bg-primary/10 hover:text-foreground",
                )}
              >
                {t.label}
              </button>
              {t.id === "github" && (
                <div className="ml-4 flex flex-col border-l border-border/60 pl-1">
                  {GITHUB_SUBTABS.map((s) => {
                    const active = tab === "github" && githubSub === s.id;
                    return (
                      <button
                        key={s.id}
                        aria-pressed={active}
                        onClick={() => {
                          setTab("github");
                          setGithubSub(s.id);
                        }}
                        className={cn(
                          "relative rounded-md px-3 py-[0.3rem] text-left text-[0.8rem]",
                          active
                            ? "font-medium text-primary before:absolute before:-left-[5px] before:top-1/2 before:h-4 before:w-[2px] before:-translate-y-1/2 before:rounded-full before:bg-primary"
                            : "text-muted-foreground hover:text-foreground",
                        )}
                      >
                        {s.label}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          ))}
        </div>
      </nav>

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto p-6">
        <div className="mx-auto w-full max-w-[640px]">
          {statusError && (
            <div role="status" className="mb-3 text-[0.8rem] text-err">
              {statusText}
            </div>
          )}
          {tab === "general" && <GeneralTab />}

          {tab === "keys" && (
            <>
              <h2 className="text-[1.05rem] font-semibold">API Keys</h2>
              <p className="mb-4 text-[0.8rem] text-muted-foreground">
                Stored in ~/.infer/auth.yaml and passed to the agent as environment variables.
              </p>
              <div className="flex flex-col gap-[0.7rem]">
                {PROVIDERS.map((p) => (
                  <div key={p.env} className="flex flex-col gap-1">
                    <Label htmlFor={p.env} className="text-[0.8rem] text-muted-foreground">
                      {p.label}
                    </Label>
                    <div className="relative">
                      <Input
                        id={p.env}
                        type={shown[p.env] ? "text" : "password"}
                        autoComplete="off"
                        placeholder={p.env}
                        value={values[p.env] ?? ""}
                        onChange={(e) => setValues((v) => ({ ...v, [p.env]: e.target.value }))}
                        className="pr-9"
                      />
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-sm"
                        aria-label={shown[p.env] ? "Hide API key" : "Show API key"}
                        onClick={() => setShown((s) => ({ ...s, [p.env]: !s[p.env] }))}
                        className="absolute right-0.5 top-1/2 -translate-y-1/2 text-muted-foreground"
                      >
                        {shown[p.env] ? <EyeOff /> : <Eye />}
                      </Button>
                    </div>
                  </div>
                ))}
              </div>
              <div className="mt-6 flex items-center gap-2 border-t border-border pt-4">
                <Button onClick={onSave}>Save</Button>
              </div>
            </>
          )}

          {tab === "updates" && (
            <>
              <h2 className="text-[1.05rem] font-semibold">Updates</h2>
              <p className="mb-4 text-[0.8rem] text-muted-foreground">Checked on launch and every 6 hours.</p>
              <div>
                {updates.map((u) => {
                  const latest = u.latest ? (u.outdated ? `→ ${u.latest}` : "up to date") : "unknown";
                  const versions = selectableVersions(u);
                  const selected = selectedVersions[u.name] ?? null;
                  const action = selected ? versionAction(u.current, selected) : null;
                  return (
                    <div
                      key={u.name}
                      className="mb-[0.3rem] flex items-center gap-2 text-[0.8rem] text-muted-foreground"
                    >
                      <span>{`${u.name} ${u.current} ${latest}`}</span>
                      {versions.length > 0 && (
                        <select
                          id={`update-${u.name.toLowerCase()}`}
                          aria-label={`Select ${u.name} version`}
                          value={selected ?? ""}
                          onChange={(e) => selectVersion(u.name, e.target.value || null)}
                          className="w-36 rounded border border-border bg-secondary px-2 py-0.5 text-[0.8rem] text-foreground"
                        >
                          <option value="">Pick a version</option>
                          {versions.map((v) => {
                            const version = stripV(v);
                            return (
                              <option key={v} value={v}>
                                {`${version}${version === u.current ? " (installed)" : ""}`}
                              </option>
                            );
                          })}
                        </select>
                      )}
                      {action && (
                        <Button
                          size="sm"
                          aria-label={`${action} ${u.name}`}
                          onClick={() => {
                            installUpdate(u.name);
                          }}
                        >
                          {action}
                        </Button>
                      )}
                    </div>
                  );
                })}
              </div>
              <div className="mt-2 flex items-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  disabled={checking}
                  onClick={async () => {
                    setChecking(true);
                    await checkForUpdates(true);
                    setChecking(false);
                  }}
                >
                  {checking ? "Checking..." : "Check for updates"}
                </Button>
                {updateButton !== "hidden" && (
                  <Button
                    size="sm"
                    disabled={updateButton === "busy"}
                    onClick={() => {
                      setCurrentView("chat");
                      applyUpdates();
                    }}
                  >
                    {updateButton === "busy" ? "Updating..." : "Install updates"}
                  </Button>
                )}
              </div>
            </>
          )}

          {tab === "agents" && <AgentsTab />}
          {tab === "secrets" && <SecretsTab />}
          {tab === "mcps" && <McpTab />}
          {tab === "compute" && <ComputeTab />}
          {tab === "workspaces" && <WorkspacesTab />}
          {tab === "github" && <GithubTab sub={githubSub} />}
          {tab === "projects" && <ProjectsTab />}
          {tab === "snippets" && <SnippetsTab />}
          {tab === "prompt" && <SystemPromptTab />}
          {tab === "skills" && <SkillsTab />}
          {tab === "voice" && <VoiceSamplesTab />}
          {tab === "avatars" && <AvatarsTab />}
        </div>
      </div>
    </div>
  );
}
