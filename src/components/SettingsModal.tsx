import { useEffect, useState } from "react";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { PROVIDERS, useDesktop } from "@/store";

export function SettingsModal() {
  const { settingsOpen, closeSettings, getAuth, saveSettings, updates, checkForUpdates, applyUpdates, showUpdateBanner } =
    useDesktop();
  const [values, setValues] = useState<Record<string, string>>({});
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    if (!settingsOpen) return;
    getAuth()
      .then((auth) => {
        const next: Record<string, string> = {};
        for (const p of PROVIDERS) next[p.env] = (auth && auth[p.env]) || "";
        setValues(next);
      })
      .catch(() => {});
  }, [settingsOpen, getAuth]);

  const onSave = () => {
    const keys: Record<string, string> = {};
    for (const p of PROVIDERS) keys[p.env] = (values[p.env] || "").trim();
    saveSettings(keys);
  };

  return (
    <Dialog
      open={settingsOpen}
      onOpenChange={(open) => {
        if (!open) closeSettings();
      }}
    >
      <DialogContent aria-label="Settings" className="max-h-[86vh] overflow-y-auto sm:max-w-[460px]">
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
        </DialogHeader>

        <div>
          <h2 className="text-[1.05rem] font-semibold">API Keys</h2>
          <p className="mb-4 text-[0.8rem] text-muted-foreground">
            Stored in ~/.infer/auth.json and passed to the agent as environment variables.
          </p>
          <div className="flex flex-col gap-[0.7rem]">
            {PROVIDERS.map((p) => (
              <div key={p.env} className="flex flex-col gap-1">
                <Label htmlFor={p.env} className="text-[0.8rem] text-muted-foreground">
                  {p.label}
                </Label>
                <Input
                  id={p.env}
                  type="password"
                  autoComplete="off"
                  placeholder={p.env}
                  value={values[p.env] ?? ""}
                  onChange={(e) => setValues((v) => ({ ...v, [p.env]: e.target.value }))}
                />
              </div>
            ))}
          </div>

          <h2 className="mt-5 text-[1.05rem] font-semibold">Updates</h2>
          <p className="mb-4 text-[0.8rem] text-muted-foreground">Checked automatically every 6 hours.</p>
          <div>
            {updates.map((u) => {
              const latest = u.latest ? (u.outdated ? `→ ${u.latest}` : "up to date") : "unknown";
              return (
                <div key={u.name} className="mb-[0.3rem] text-[0.8rem] text-muted-foreground">
                  {`${u.name} ${u.current} ${latest}`}
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
            {showUpdateBanner && (
              <Button
                size="sm"
                onClick={() => {
                  closeSettings();
                  applyUpdates();
                }}
              >
                Install updates
              </Button>
            )}
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={closeSettings}>
            Cancel
          </Button>
          <Button onClick={onSave}>Save</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
