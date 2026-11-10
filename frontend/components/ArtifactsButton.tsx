import { useCallback, useState } from "react";
import { Download, Package, RefreshCw } from "lucide-react";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api, type Artifact } from "@/lib/tauri";
import { useDesktop } from "@/store";
import { useSidebar } from "@/stores/sidebar";

const ICON_BUTTON =
  "inline-flex size-7 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:bg-secondary hover:text-foreground";

const formatSize = (bytes: number) =>
  bytes < 1024
    ? `${bytes} B`
    : bytes < 1024 ** 2
      ? `${(bytes / 1024).toFixed(1)} KB`
      : `${(bytes / 1024 ** 2).toFixed(1)} MB`;

/** The files the open chat produced, wherever it runs, each saved to Downloads on request. */
export function ArtifactsButton() {
  const { projects } = useSidebar();
  const { sessionId } = useDesktop();
  const [open, setOpen] = useState(false);
  const [artifacts, setArtifacts] = useState<Artifact[] | null>(null);
  const [status, setStatus] = useState("");
  const project = sessionId ? projects[sessionId] : undefined;

  const load = useCallback(async () => {
    if (!sessionId) return;
    setArtifacts(null);
    setStatus("");
    try {
      setArtifacts(await api.listArtifacts(sessionId, project));
    } catch (e) {
      setArtifacts([]);
      setStatus(String(e));
    }
  }, [sessionId, project]);

  const save = async (name: string) => {
    if (!sessionId) return;
    setStatus(`Saving ${name}...`);
    try {
      setStatus(`Saved to ${await api.saveArtifact(sessionId, project, name)}`);
    } catch (e) {
      setStatus(String(e));
    }
  };

  if (!sessionId) return null;
  return (
    <>
      <button
        aria-label="Artifacts"
        title="Artifacts - files this chat produced"
        className={ICON_BUTTON}
        onClick={() => {
          setOpen(true);
          load();
        }}
      >
        <Package size={16} aria-hidden="true" />
      </button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-1">
              Artifacts
              <button aria-label="Reload artifacts" title="Reload artifacts" className={ICON_BUTTON} onClick={load}>
                <RefreshCw size={14} aria-hidden="true" />
              </button>
            </DialogTitle>
            <DialogDescription>Files this chat produced. Saving one puts it in Downloads.</DialogDescription>
          </DialogHeader>
          {artifacts === null && <p className="text-muted-foreground">Loading...</p>}
          {artifacts?.length === 0 && !status && <p className="text-muted-foreground">No artifacts yet.</p>}
          <ul className="flex max-h-72 flex-col gap-1 overflow-y-auto">
            {artifacts?.map((artifact) => (
              <li key={artifact.name} className="flex items-center gap-2">
                <span className="min-w-0 flex-1 truncate" title={artifact.name}>
                  {artifact.name}
                </span>
                <span className="shrink-0 text-[0.75rem] text-muted-foreground">
                  {formatSize(artifact.size)}
                  {artifact.modified_at && ` - ${new Date(artifact.modified_at).toLocaleString()}`}
                </span>
                <button
                  aria-label={`Save artifact ${artifact.name}`}
                  title={`Save ${artifact.name} to Downloads`}
                  className={ICON_BUTTON}
                  onClick={() => save(artifact.name)}
                >
                  <Download size={14} aria-hidden="true" />
                </button>
              </li>
            ))}
          </ul>
          {status && (
            <p aria-live="polite" className="break-all text-[0.8rem] text-muted-foreground">
              {status}
            </p>
          )}
        </DialogContent>
      </Dialog>
    </>
  );
}
