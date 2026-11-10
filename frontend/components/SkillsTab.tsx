import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { api } from "@/lib/tauri";
import {
  DEFAULT_REGISTRY_URL,
  fetchSkillsCatalog,
  getRegistryUrl,
  setRegistryUrl,
  type SkillsCatalog,
} from "@/lib/skills";

export function SkillsTab() {
  const [catalog, setCatalog] = useState<SkillsCatalog | null>(null);
  const [loading, setLoading] = useState(true);
  const [err, setErr] = useState("");
  const [search, setSearch] = useState("");
  const [registryUrl, setRegistryUrlState] = useState(getRegistryUrl());
  const [urlDirty, setUrlDirty] = useState(false);
  const [installed, setInstalled] = useState<Set<string>>(new Set());
  const [installingName, setInstallingName] = useState("");
  const [installErr, setInstallErr] = useState<{ name: string; msg: string } | null>(null);

  useEffect(() => {
    fetchSkillsCatalog()
      .then(setCatalog)
      .catch((e) => setErr(String(e)))
      .finally(() => setLoading(false));
    api
      .listInstalledSkills()
      .then((s) => setInstalled(new Set(s)))
      .catch(() => {});
  }, []);

  const runSkillAction = async (name: string, action: (name: string) => Promise<void>) => {
    setInstallingName(name);
    setInstallErr(null);
    try {
      await action(name);
      setInstalled(new Set(await api.listInstalledSkills()));
    } catch (e) {
      setInstallErr({ name, msg: String(e) });
    } finally {
      setInstallingName("");
    }
  };

  const filtered = useMemo(() => {
    if (!catalog) return [];
    const q = search.toLowerCase();
    if (!q) return catalog.skills;
    return catalog.skills.filter((s) => s.name.toLowerCase().includes(q) || s.description.toLowerCase().includes(q));
  }, [catalog, search]);

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">Skills</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        Remote skills registry. Type / in the composer to discover and invoke skills. Remote skills are downloaded on
        first use with your approval.
      </p>

      <h3 className="text-[0.9rem] font-semibold">Registry URL</h3>
      <div className="mb-4 flex items-center gap-2">
        <Input
          value={registryUrl}
          onChange={(e) => {
            setRegistryUrlState(e.target.value);
            setUrlDirty(true);
          }}
          placeholder={DEFAULT_REGISTRY_URL}
          className="flex-1"
        />
        {urlDirty && (
          <Button
            size="sm"
            onClick={() => {
              setRegistryUrl(registryUrl);
              setUrlDirty(false);
              setLoading(true);
              setErr("");
              fetchSkillsCatalog(registryUrl)
                .then(setCatalog)
                .catch((e) => setErr(String(e)))
                .finally(() => setLoading(false));
            }}
          >
            Apply
          </Button>
        )}
      </div>

      <h3 className="text-[0.9rem] font-semibold">Catalog</h3>
      {loading ? (
        <p className="text-[0.8rem] text-muted-foreground">Loading catalog...</p>
      ) : err ? (
        <p className="text-[0.8rem] text-err">Couldn't load catalog: {err}</p>
      ) : catalog ? (
        <>
          <p className="mb-3 text-[0.75rem] text-muted-foreground">
            Version <code className="rounded bg-secondary px-1">{catalog.version}</code> — {catalog.skills.length}{" "}
            skills available.
          </p>

          <Input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search skills by name or description..."
            className="mb-3"
          />

          <div className="flex flex-col gap-1">
            {filtered.map((s) => {
              const isInstalled = installed.has(s.name);
              return (
                <div
                  key={s.name}
                  className={cn(
                    "flex items-center gap-2 rounded-md border bg-card px-3 py-2 text-[0.82rem]",
                    isInstalled ? "border-primary" : "border-border",
                  )}
                >
                  <span className="min-w-0 flex-1">
                    <span className="font-medium">{s.name}</span>
                    {s.version && <span className="ml-1 text-[0.7rem] text-muted-foreground">v{s.version}</span>}
                    {s.description && (
                      <p className="line-clamp-1 text-[0.73rem] text-muted-foreground">{s.description}</p>
                    )}
                    {installErr?.name === s.name && <p className="text-[0.73rem] text-err">{installErr.msg}</p>}
                  </span>
                  {isInstalled ? (
                    <>
                      <span className="shrink-0 rounded bg-secondary px-1.5 py-0.5 text-[0.65rem] text-muted-foreground">
                        installed
                      </span>
                      <Button
                        size="sm"
                        variant="outline"
                        className="shrink-0"
                        disabled={installingName !== ""}
                        onClick={() => runSkillAction(s.name, api.uninstallSkill)}
                      >
                        {installingName === s.name ? "Uninstalling..." : "Uninstall"}
                      </Button>
                    </>
                  ) : (
                    <Button
                      size="sm"
                      variant="outline"
                      className="shrink-0"
                      disabled={installingName !== ""}
                      onClick={() => runSkillAction(s.name, api.installSkill)}
                    >
                      {installingName === s.name ? "Installing..." : "Install"}
                    </Button>
                  )}
                </div>
              );
            })}
            {filtered.length === 0 && (
              <p className="text-[0.8rem] text-muted-foreground">No skills match your search.</p>
            )}
          </div>
        </>
      ) : (
        <p className="text-[0.8rem] text-muted-foreground">No catalog loaded.</p>
      )}
    </>
  );
}
