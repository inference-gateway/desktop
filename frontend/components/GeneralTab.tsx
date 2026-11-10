import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, CheckCircle2, CircleMinus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import {
  api,
  DEFAULT_SCREEN_RECORD_KEEP,
  SCREEN_RECORD_KEEP_KEY,
  isMacOS,
  screenRecordKeep,
  type ComputerUsePermissionStatus,
  type DesktopConfig,
  type OsPermissionState,
} from "@/lib/tauri";
import { useBrowserUseStatus } from "@/components/BrowserBridgeDot";
import { useDesktop } from "@/store";
import { useSidebar } from "@/stores/sidebar";
import { useBackend } from "@/stores/backend";
import { useSnippets } from "@/stores/snippets";
import { DEFAULT_CONFIG, type StorageField } from "@/lib/desktop-config";

const STORAGE_BACKENDS = ["jsonl", "sqlite", "postgres", "redis", "d1"] as const;

// Fields per backend, mapping 1:1 to infer's storage.<type>.* schema.
const STORAGE_FIELDS: Record<string, readonly StorageField[]> = {
  jsonl: [
    {
      key: "storage_directory",
      label: "Conversations directory",
      ph: "default: ~/.infer/projects/<project>/conversations",
    },
  ],
  sqlite: [{ key: "sqlite_path", label: "Database file", ph: "default: ~/.infer/conversations.db" }],
  postgres: [
    { key: "postgres_host", label: "Host", ph: "localhost" },
    { key: "postgres_port", label: "Port", ph: "5432" },
    { key: "postgres_database", label: "Database", ph: "infer_conversations" },
    { key: "postgres_username", label: "Username" },
    { key: "postgres_password", label: "Password", secret: true },
    {
      key: "postgres_ssl_mode",
      label: "SSL mode",
      options: ["disable", "allow", "prefer", "require", "verify-ca", "verify-full"],
    },
  ],
  redis: [
    { key: "redis_host", label: "Host", ph: "localhost" },
    { key: "redis_port", label: "Port", ph: "6379" },
    { key: "redis_password", label: "Password", secret: true },
    { key: "redis_db", label: "Database number", ph: "0" },
  ],
  d1: [
    { key: "d1_account_id", label: "Account ID" },
    { key: "d1_database_id", label: "Database ID" },
    { key: "d1_api_token", label: "API token", secret: true },
    { key: "d1_base_url", label: "Base URL", ph: "https://api.cloudflare.com/client/v4" },
  ],
};

export function GeneralTab() {
  const { models, model, setModel } = useBackend();
  const { maxSessions, setMaxSessions, setShowStatusBar } = useDesktop();
  const [config, setConfigs] = useState<DesktopConfig>({ ...DEFAULT_CONFIG });
  const [computerUsePermissions, setComputerUsePermissions] = useState<ComputerUsePermissionStatus | null>(null);
  const browserUse = useBrowserUseStatus();
  const [browserUseError, setBrowserUseError] = useState("");
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");
  const [savedTts, setSavedTts] = useState(false);
  const [savedTtv, setSavedTtv] = useState(false);
  const [recordKeep, setRecordKeepState] = useState(screenRecordKeep());

  const setRecordKeep = (n: number) => {
    const v = Number.isFinite(n) && n >= 1 ? Math.floor(n) : DEFAULT_SCREEN_RECORD_KEEP;
    setRecordKeepState(v);
    localStorage.setItem(SCREEN_RECORD_KEEP_KEY, String(v));
  };

  useEffect(() => {
    api
      .getConfig()
      .then((c) => {
        setConfigs(c);
        setSavedTts(c.text_to_speech_enabled);
        setSavedTtv(c.text_to_video_enabled);
      })
      .catch(() => {});
  }, []);

  const refreshComputerUsePermissions = useCallback(() => {
    api
      .computerUsePermissionStatus()
      .then(setComputerUsePermissions)
      .catch(() => {});
  }, []);

  useEffect(() => {
    refreshComputerUsePermissions();
    window.addEventListener("focus", refreshComputerUsePermissions);
    return () => window.removeEventListener("focus", refreshComputerUsePermissions);
  }, [refreshComputerUsePermissions]);

  const apply = async () => {
    setSaving(true);
    try {
      await api.setConfig(config);
      setShowStatusBar(config.status_bar_enabled);
      setDirty(false);
      setSaved(true);
      setError("");
      // The gateway reads AUDIO_* and VIDEOS_ENABLED env at spawn, so a TTS or
      // TTV toggle needs a restart (same path as saving an API key).
      if (savedTts !== config.text_to_speech_enabled || savedTtv !== config.text_to_video_enabled) {
        setSavedTts(config.text_to_speech_enabled);
        setSavedTtv(config.text_to_video_enabled);
        await api.startGateway(false, true);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const set = (k: keyof DesktopConfig, v: string | boolean) => {
    setConfigs((c) => ({ ...c, [k]: v }));
    setDirty(true);
    setSaved(false);
    setError("");
  };

  const storageFields = STORAGE_FIELDS[config.storage_backend];

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">General</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        Agent sessions, storage backend, default model, and gateway settings. Changes take effect on next agent spawn.
      </p>

      <h3 className="text-[0.9rem] font-semibold">Computer Use</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Lets the agent see the screen and control other apps. Off by default; takes effect on the next agent spawn.
      </p>
      <div className="mb-5 flex items-center gap-3">
        <input
          type="checkbox"
          id="computer-use-enabled"
          checked={!!computerUsePermissions?.computer_use_enabled}
          onChange={(e) => {
            api
              .setComputerUseEnabled(e.target.checked)
              .catch(() => {})
              .then(refreshComputerUsePermissions);
          }}
          className="h-4 w-4 accent-primary"
        />
        <Label htmlFor="computer-use-enabled" className="cursor-pointer text-[0.8rem] font-medium">
          Enable Computer Use
        </Label>
      </div>

      {computerUsePermissions?.computer_use_enabled && (
        <div className="mb-5 flex flex-col gap-3">
          <ComputerUsePermissionSection
            title="Accessibility"
            state={computerUsePermissions.accessibility}
            warning="Accessibility access is not granted. Computer Use cannot inspect or control other apps. Click Grant, then enable Inference Gateway Desktop in the prompt."
            onGrant={() => {
              api
                .requestAccessibilityPermission()
                .catch(() => {})
                .then(refreshComputerUsePermissions);
            }}
          />
          <ComputerUsePermissionSection
            title="Screen Recording"
            state={computerUsePermissions.screen_recording}
            warning="Screen Recording access is not granted. Computer Use cannot capture the screen. Click Grant to request access - macOS applies Screen Recording after the app restarts."
            onGrant={() => {
              api
                .requestScreenRecordingPermission()
                .catch(() => {})
                .then(refreshComputerUsePermissions);
            }}
          />
        </div>
      )}

      {/* Browser use */}
      <h3 className="text-[0.9rem] font-semibold">Browser Use</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Lets the agent drive your real browser through the opentask extension. Paste the port and token below into the
        extension options. The infer daemon the app runs its chats through holds the port and reports when the extension
        connects.
      </p>
      <div className="mb-3 flex items-center gap-3">
        <input
          type="checkbox"
          id="browser-use-enabled"
          checked={!!browserUse?.enabled}
          onChange={(e) => {
            setBrowserUseError("");
            api.setBrowserUseEnabled(e.target.checked).catch((err) => setBrowserUseError(String(err)));
          }}
          className="h-4 w-4 accent-primary"
        />
        <Label htmlFor="browser-use-enabled" className="cursor-pointer text-[0.8rem] font-medium">
          Enable Browser Use (extension)
        </Label>
      </div>
      {browserUseError && (
        <p role="alert" className="mb-3 text-[0.75rem] text-destructive">
          {browserUseError}
        </p>
      )}
      {browserUse?.enabled && (
        <div className="mb-5 flex flex-col gap-2 text-[0.8rem]">
          <div className="flex items-center gap-2">
            <span
              className={cn(
                "h-[0.45rem] w-[0.45rem] shrink-0 rounded-full",
                browserUse.connected ? "bg-emerald-500" : "bg-muted-foreground",
              )}
            />
            {browserUse.connected ? "Extension connected" : "Extension not connected"}
          </div>
          <div className="flex items-center gap-2">
            <Label className="w-12">Port</Label>
            <Input readOnly value={browserUse.port} className="w-28" />
          </div>
          <div className="flex items-center gap-2">
            <Label className="w-12">Token</Label>
            <Input readOnly value={browserUse.token} className="max-w-[24rem]" />
            <Button variant="outline" size="sm" onClick={() => navigator.clipboard.writeText(browserUse.token)}>
              Copy
            </Button>
          </div>
        </div>
      )}

      {/* Agent sessions */}
      <h3 className="mt-5 text-[0.9rem] font-semibold">Sessions</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Run multiple agent sessions at once. Each session is a separate infer agent process.
      </p>
      <div className="mb-5 flex flex-col gap-1">
        <Label htmlFor="max-sessions" className="text-[0.8rem] text-muted-foreground">
          Max concurrent sessions
        </Label>
        <Input
          id="max-sessions"
          type="number"
          min={1}
          value={maxSessions}
          onChange={(e) => setMaxSessions(parseInt(e.target.value, 10))}
          className="w-24"
        />
      </div>

      {/* Screen recording */}
      {isMacOS && (
        <div className="mt-5 mb-5 flex flex-col gap-1">
          <h3 className="text-[0.9rem] font-semibold">Screen recording</h3>
          <p className="mb-3 text-[0.75rem] text-muted-foreground">
            Recordings from the top-bar Record button. They capture everything typed while recording, including
            passwords.
          </p>
          <Label htmlFor="screen-record-keep" className="text-[0.8rem] text-muted-foreground">
            Screen recordings to keep
          </Label>
          <Input
            id="screen-record-keep"
            type="number"
            min={1}
            value={recordKeep}
            onChange={(e) => setRecordKeep(parseInt(e.target.value, 10))}
            className="w-24"
          />
        </div>
      )}

      {/* Storage */}
      <h3 className="mt-5 text-[0.9rem] font-semibold">Storage</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Where conversations are stored. Pick a backend and fill in its connection details.
      </p>
      <div className="mb-3 flex flex-col gap-1">
        <Label htmlFor="storage-backend" className="text-[0.8rem] text-muted-foreground">
          Backend
        </Label>
        <select
          id="storage-backend"
          value={config.storage_backend}
          onChange={(e) => set("storage_backend", e.target.value)}
          className="w-44 rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
        >
          {STORAGE_BACKENDS.map((b) => (
            <option key={b} value={b}>
              {b}
            </option>
          ))}
        </select>
      </div>
      {storageFields ? (
        <div className="mb-3 flex flex-col gap-3">
          {storageFields.map((f) => (
            <div key={f.key} className="flex flex-col gap-1">
              <Label htmlFor={f.key} className="text-[0.8rem] text-muted-foreground">
                {f.label}
              </Label>
              {f.options ? (
                <select
                  id={f.key}
                  value={config[f.key] as string}
                  onChange={(e) => set(f.key, e.target.value)}
                  className="w-44 rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
                >
                  {f.options.map((o) => (
                    <option key={o} value={o}>
                      {o}
                    </option>
                  ))}
                </select>
              ) : (
                <Input
                  id={f.key}
                  type={f.secret ? "password" : undefined}
                  value={config[f.key] as string}
                  onChange={(e) => set(f.key, e.target.value)}
                  placeholder={f.ph}
                />
              )}
            </div>
          ))}
        </div>
      ) : (
        <p className="mb-3 text-[0.75rem] text-muted-foreground">
          The {config.storage_backend} backend is not editable here yet.
        </p>
      )}

      {/* Default model */}
      <h3 className="mt-5 text-[0.9rem] font-semibold">Default Model</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Model selected by default for new chats. Persisted server-side so it survives a cache clear.
      </p>
      <div className="mb-3 flex flex-col gap-1">
        <Label htmlFor="default-model" className="text-[0.8rem] text-muted-foreground">
          Model
        </Label>
        <select
          id="default-model"
          value={model}
          onChange={(e) => setModel(e.target.value)}
          className="w-full max-w-xs rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
        >
          {(models.includes(model) ? models : [model, ...models].filter(Boolean)).map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
      </div>

      {/* Gateway */}
      <h3 className="mt-5 text-[0.9rem] font-semibold">Gateway</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        URL of the inference gateway. Changes take effect on the next gateway restart.
      </p>
      <div className="mb-3 flex flex-col gap-1">
        <Label htmlFor="gateway-url" className="text-[0.8rem] text-muted-foreground">
          Gateway URL
        </Label>
        <Input
          id="gateway-url"
          value={config.gateway_url}
          onChange={(e) => set("gateway_url", e.target.value)}
          placeholder="http://localhost:8080"
        />
      </div>

      {/* Status bar */}
      <h3 className="mt-5 text-[0.9rem] font-semibold">Status bar</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        The row above the composer with agent state, tool count and token usage. Shared with the CLI status bar
        (chat.status_bar.enabled).
      </p>
      <div className="mb-5 flex items-center gap-3">
        <input
          type="checkbox"
          id="status-bar-enabled"
          checked={config.status_bar_enabled}
          onChange={(e) => set("status_bar_enabled", e.target.checked)}
          className="h-4 w-4 accent-primary"
        />
        <Label htmlFor="status-bar-enabled" className="cursor-pointer text-[0.8rem] font-medium">
          Show status bar
        </Label>
      </div>

      {/* Next-step suggestions */}
      <h3 className="mt-5 text-[0.9rem] font-semibold">Next-step suggestions</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        After each reply, the CLI suggests a one-line next step in the empty composer. Tab accepts it, typing or Escape
        dismisses it. Each suggestion is one extra model call (chat.suggestions.enabled).
      </p>
      <div className="mb-5 flex items-center gap-3">
        <input
          type="checkbox"
          id="next-step-suggestions-enabled"
          checked={config.next_step_suggestions_enabled}
          onChange={(e) => set("next_step_suggestions_enabled", e.target.checked)}
          className="h-4 w-4 accent-primary"
        />
        <Label htmlFor="next-step-suggestions-enabled" className="cursor-pointer text-[0.8rem] font-medium">
          Suggest a next step after each reply
        </Label>
      </div>

      {/* Text to speech */}
      <h3 className="mt-5 text-[0.9rem] font-semibold">Text to speech</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Lets the agent generate spoken audio with the CLI's TextToSpeech tool. Off by default; saving restarts the
        gateway. Manage reference recordings in the Voice samples tab.
      </p>
      <div className="mb-5 flex items-center gap-3">
        <input
          type="checkbox"
          id="tts-enabled"
          checked={config.text_to_speech_enabled}
          onChange={(e) => set("text_to_speech_enabled", e.target.checked)}
          className="h-4 w-4 accent-primary"
        />
        <Label htmlFor="tts-enabled" className="cursor-pointer text-[0.8rem] font-medium">
          Enable Text to Speech
        </Label>
      </div>

      {/* Text to video */}
      <h3 className="mt-5 text-[0.9rem] font-semibold">Text to video</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Lets the agent render lip-synced talking clips from your avatars with the CLI's TextToVideo tool. Off by
        default: every render sends the avatar's portrait and the voice clip to the video provider (ElevenLabs), so add
        an ElevenLabs key under API Keys. Saving restarts the gateway. Manage portraits in the Avatars tab.
      </p>
      <div className="mb-5 flex items-center gap-3">
        <input
          type="checkbox"
          id="ttv-enabled"
          checked={config.text_to_video_enabled}
          onChange={(e) => set("text_to_video_enabled", e.target.checked)}
          className="h-4 w-4 accent-primary"
        />
        <Label htmlFor="ttv-enabled" className="cursor-pointer text-[0.8rem] font-medium">
          Enable Text to Video
        </Label>
      </div>

      {/* Vision */}
      <h3 className="mt-5 text-[0.9rem] font-semibold">Vision</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Vision model the agent uses to describe images and video frames (the CLI's ImageDecode tool). Blank disables it.
        For a fully local setup run Ollama, pull the model, and add OLLAMA_API_URL=http://localhost:11434/v1 under
        Providers.
      </p>
      <div className="mb-5 flex flex-col gap-1">
        <Label htmlFor="vision-annotator-model" className="text-[0.8rem] text-muted-foreground">
          Vision model
        </Label>
        <Input
          id="vision-annotator-model"
          value={config.vision_annotator_model}
          onChange={(e) => set("vision_annotator_model", e.target.value)}
          placeholder="ollama/qwen3-vl:2b"
        />
      </div>

      {/* Export/import */}
      <ConfigTransfer />

      <div className="sticky bottom-0 mt-6 flex items-center gap-3 border-t border-border bg-background/95 pb-1 pt-4 backdrop-blur">
        <Button onClick={apply} disabled={!dirty || saving}>
          {saving ? "Saving..." : "Save config"}
        </Button>
        {dirty && !error && <span className="text-[0.8rem] text-muted-foreground">Unsaved changes</span>}
        {saved && !dirty && (
          <span role="status" className="text-[0.8rem] text-muted-foreground">
            Saved
          </span>
        )}
        {error && (
          <span role="status" className="text-[0.8rem] text-err">
            Couldn't save: {error}
          </span>
        )}
      </div>
    </>
  );
}

function ConfigTransfer() {
  const { reloadDesktopData } = useSnippets();
  const { loadProjects } = useSidebar();
  const [format, setFormat] = useState("json");
  const [target, setTarget] = useState<"file" | "github">("file");
  const [repo, setRepo] = useState("");
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");

  const asNotice = (base: string, warnings: string[]) =>
    warnings.length ? `${base} (warnings: ${warnings.join("; ")})` : base;

  const exportConfig = async () => {
    setBusy(true);
    setError("");
    setMessage("");
    try {
      const r =
        target === "file"
          ? await api.exportDesktopFile(format)
          : await api.exportDesktopGithub(repo.trim(), format, token.trim());
      if (r.location) setMessage(asNotice(`Exported to ${r.location}`, r.warnings));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const importConfig = async () => {
    setBusy(true);
    setError("");
    setMessage("");
    try {
      const r =
        target === "file" ? await api.importDesktopFile() : await api.importDesktopGithub(repo.trim(), token.trim());
      await reloadDesktopData();
      await loadProjects();
      setMessage(asNotice(`Imported: ${r.imported.join(", ") || "nothing"}`, r.warnings));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const disabled = busy || (target === "github" && repo.trim().length === 0);

  return (
    <>
      <h3 className="mt-5 text-[0.9rem] font-semibold">Export / Import</h3>
      <p className="mb-3 text-[0.75rem] text-muted-foreground">
        Move the full desktop state - Settings, projects, A2A agents, scheduled jobs, snippets and skills - between
        machines. Credentials are never exported.
      </p>
      <div className="mb-3 flex flex-col gap-1">
        <Label htmlFor="export-format" className="text-[0.8rem] text-muted-foreground">
          Format
        </Label>
        <select
          id="export-format"
          value={format}
          onChange={(e) => setFormat(e.target.value)}
          className="w-44 rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
        >
          <option value="json">json</option>
          <option value="yaml">yaml</option>
          <option value="toml">toml</option>
        </select>
      </div>
      <div className="mb-3 flex flex-col gap-1">
        <Label htmlFor="export-destination" className="text-[0.8rem] text-muted-foreground">
          Destination
        </Label>
        <select
          id="export-destination"
          value={target}
          onChange={(e) => setTarget(e.target.value as "file" | "github")}
          className="w-44 rounded border border-border bg-secondary px-2 py-1.5 text-[0.85rem] text-foreground"
        >
          <option value="file">Local file (native dialog)</option>
          <option value="github">Private GitHub repo</option>
        </select>
      </div>
      {target === "github" && (
        <>
          <div className="mb-3 flex flex-col gap-1">
            <Label htmlFor="export-repo" className="text-[0.8rem] text-muted-foreground">
              Repository (owner/repo)
            </Label>
            <Input
              id="export-repo"
              value={repo}
              onChange={(e) => setRepo(e.target.value)}
              placeholder="you/desktop-config"
            />
          </div>
          <div className="mb-3 flex flex-col gap-1">
            <Label htmlFor="export-token" className="text-[0.8rem] text-muted-foreground">
              GitHub token (optional)
            </Label>
            <Input
              id="export-token"
              type="password"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="empty = gh's stored login"
            />
          </div>
        </>
      )}
      <div className="mb-3 flex flex-wrap items-center gap-3">
        <Button
          onClick={() => {
            void exportConfig();
          }}
          disabled={disabled}
        >
          Export config
        </Button>
        <Button
          onClick={() => {
            void importConfig();
          }}
          disabled={disabled}
          variant="secondary"
        >
          Import config
        </Button>
        {message && (
          <span role="status" className="text-[0.75rem] text-muted-foreground">
            {message}
          </span>
        )}
        {error && (
          <span role="status" className="text-[0.8rem] text-err">
            {error}
          </span>
        )}
      </div>
    </>
  );
}

function ComputerUsePermissionSection({
  title,
  state,
  warning,
  onGrant,
}: {
  title: string;
  state: OsPermissionState;
  warning: string;
  onGrant?: () => void;
}) {
  const isWarning = state === "not_granted" || state === "unavailable";
  const status = {
    granted: "Granted",
    not_granted: "Not granted",
    unavailable: "Unavailable in development",
    not_applicable: "Not required on this OS",
  }[state];
  const message =
    state === "unavailable"
      ? `${title} status cannot be verified from Desktop dev because macOS grants access to app bundle identities. Build and launch Inference Gateway Desktop.app to check this permission.`
      : state === "not_granted"
        ? warning
        : null;

  return (
    <section className="rounded-lg border border-border bg-secondary/40 p-3">
      <div className="flex items-center gap-2">
        {isWarning ? (
          <AlertTriangle className="text-amber-500" size={17} aria-hidden="true" />
        ) : state === "granted" ? (
          <CheckCircle2 className="text-emerald-500" size={17} aria-hidden="true" />
        ) : (
          <CircleMinus className="text-muted-foreground" size={17} aria-hidden="true" />
        )}
        <h3 className="text-[0.9rem] font-semibold">{title}</h3>
        <span className="ml-auto text-[0.75rem] text-muted-foreground">{status}</span>
      </div>
      {message && (
        <p role="alert" className="mt-2 text-[0.75rem] text-amber-600 dark:text-amber-400">
          {message}
        </p>
      )}
      {state === "not_granted" && onGrant && (
        <Button size="sm" variant="outline" className="mt-2" aria-label={`Grant ${title}`} onClick={onGrant}>
          Grant
        </Button>
      )}
    </section>
  );
}
