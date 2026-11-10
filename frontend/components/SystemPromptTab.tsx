import { useEffect, useState } from "react";
import { AlertTriangle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { api, type DesktopConfig } from "@/lib/tauri";
import { DEFAULT_CONFIG } from "@/lib/desktop-config";

export function SystemPromptTab() {
  const [config, setConfig] = useState<DesktopConfig>({ ...DEFAULT_CONFIG });
  const [overrideEnabled, setOverrideEnabled] = useState(false);
  const [showOverrideWarning, setShowOverrideWarning] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [err, setErr] = useState("");

  useEffect(() => {
    api
      .getConfig()
      .then((cfg) => {
        setConfig(cfg);
      })
      .catch(() => {});
  }, []);

  const update = (k: keyof DesktopConfig, v: string) => {
    setConfig((c) => ({ ...c, [k]: v }));
    setDirty(true);
    setSaved(false);
    setErr("");
  };

  const toggleOverride = () => {
    if (overrideEnabled) {
      setOverrideEnabled(false);
      update("system_prompt", "");
    } else {
      setShowOverrideWarning(true);
    }
  };

  const apply = async () => {
    setSaving(true);
    try {
      await api.setConfig(config);
      setDirty(false);
      setSaved(true);
      setErr("");
    } catch (e) {
      setErr(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">System Prompt</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        Customize the default system prompt sent to the agent. Extra instructions are appended to the default; override
        replaces it entirely. When both are set, the override replaces the default and extra instructions are appended.
      </p>

      <div className="mb-5 flex flex-col gap-2">
        <Label htmlFor="extra-instructions" className="text-[0.8rem] font-medium">
          Extra instructions (append mode, always active)
        </Label>
        <p className="mb-1 text-[0.75rem] text-muted-foreground">
          Appended to the default system prompt on every agent session. Safe for project conventions, tool usage
          preferences, or security constraints.
        </p>
        <textarea
          id="extra-instructions"
          rows={4}
          value={config.extra_instructions}
          onChange={(e) => update("extra_instructions", e.target.value)}
          placeholder="e.g. Always cite sources when providing information."
          className="w-full resize-y rounded-lg border border-input bg-transparent px-2.5 py-1.5 text-[0.85rem] text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
        />
      </div>

      <div className="mb-5 flex items-start gap-3 rounded-md border border-border bg-card p-4">
        <input
          type="checkbox"
          id="override-toggle"
          checked={overrideEnabled}
          onChange={toggleOverride}
          className="mt-1 h-4 w-4 shrink-0 accent-primary"
        />
        <div className="flex flex-col gap-1">
          <Label htmlFor="override-toggle" className="cursor-pointer text-[0.8rem] font-medium">
            Override system prompt
          </Label>
          <p className="text-[0.75rem] text-muted-foreground">
            Replaces the entire default system prompt, discarding built-in instructions the agent relies on, such as how
            to use its tools and workspace context like the current directory, file tree, and git branch. Prefer extra
            instructions above. When enabled, extra instructions are still appended after the override.
          </p>
        </div>
      </div>

      {overrideEnabled && (
        <div className="mb-5 flex flex-col gap-2">
          <Label htmlFor="override-prompt" className="text-[0.8rem] font-medium">
            Override prompt
          </Label>
          <p className="mb-1 text-[0.75rem] text-muted-foreground">
            Replaces the default system prompt. Extra instructions from above are still appended after this text.
          </p>
          <textarea
            id="override-prompt"
            rows={8}
            value={config.system_prompt}
            onChange={(e) => update("system_prompt", e.target.value)}
            placeholder="Write your custom system prompt here..."
            className="w-full resize-y rounded-lg border border-input bg-transparent px-2.5 py-1.5 text-[0.85rem] text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
          />
        </div>
      )}

      <Dialog open={showOverrideWarning} onOpenChange={setShowOverrideWarning}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="text-amber-500" size={18} />
              Override system prompt?
            </DialogTitle>
            <DialogDescription>
              You are overriding the default system prompt, which provides useful context for the orchestrator about the
              current project, installed plugins, skills, and memory. Prefer using "Extra instructions" (append mode)
              instead to keep this context.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowOverrideWarning(false)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                setOverrideEnabled(true);
                setShowOverrideWarning(false);
              }}
            >
              Enable override
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <div className="sticky bottom-0 mt-6 flex items-center gap-3 border-t border-border bg-background/95 pb-1 pt-4 backdrop-blur">
        <Button onClick={apply} disabled={!dirty || saving}>
          {saving ? "Saving..." : "Save config"}
        </Button>
        {dirty && !err && <span className="text-[0.8rem] text-muted-foreground">Unsaved changes</span>}
        {saved && !dirty && (
          <span role="status" className="text-[0.8rem] text-muted-foreground">
            Saved
          </span>
        )}
        {err && (
          <span role="status" className="text-[0.8rem] text-err">
            Couldn't save: {err}
          </span>
        )}
      </div>
    </>
  );
}
