import { useCallback, useEffect, useRef, useState } from "react";
import { Download, Eye, Package, Play, RefreshCw } from "lucide-react";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { markArtifactsSeen, mediaType, seenArtifacts, unseenArtifacts } from "@/lib/artifacts";
import { api, type Artifact } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { useSidebar } from "@/stores/sidebar";
import { AudioPlayer } from "./AudioPlayer";

const ICON_BUTTON =
  "inline-flex size-7 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:bg-secondary hover:text-foreground";

const POLL_MS = 5000;

type Preview = { name: string; type: string; url: string };

const formatSize = (bytes: number) =>
  bytes < 1024
    ? `${bytes} B`
    : bytes < 1024 ** 2
      ? `${(bytes / 1024).toFixed(1)} KB`
      : `${(bytes / 1024 ** 2).toFixed(1)} MB`;

/**
 * How many of the open chat's artifacts were added or changed since the user last opened the dialog.
 * shortcut: only the open chat's runs are watched, so idle chats never wake their computer - a run
 * that ended while another chat was open lights the button on its next run. Upgrade with a daemon event.
 */
function useUnseenArtifacts(sessionId: string | null, project: string | undefined, running: boolean) {
  const [unseen, setUnseen] = useState(0);
  const lastRunning = useRef<string | null>(null);

  useEffect(() => setUnseen(0), [sessionId]);

  useEffect(() => {
    if (!sessionId) return;
    const justFinished = lastRunning.current === sessionId;
    lastRunning.current = running ? sessionId : null;
    if (!running && !justFinished) return;
    let alive = true;
    let busy = false;
    const check = () => {
      if (busy) return;
      busy = true;
      api
        .listArtifacts(sessionId, project)
        .then(
          (listed) => alive && setUnseen(unseenArtifacts(seenArtifacts(sessionId), listed).length),
          () => {},
        )
        .finally(() => (busy = false));
    };
    check();
    const timer = running ? setInterval(check, POLL_MS) : undefined;
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [sessionId, project, running]);

  return [unseen, setUnseen] as const;
}

/** The files the open chat produced, wherever it runs, each saved to Downloads on request. */
export function ArtifactsButton() {
  const { projects } = useSidebar();
  const { sessionId, running } = useDesktop();
  const [open, setOpen] = useState(false);
  const [artifacts, setArtifacts] = useState<Artifact[] | null>(null);
  const [fresh, setFresh] = useState<Set<string>>(new Set());
  const [preview, setPreview] = useState<Preview | null>(null);
  const [status, setStatus] = useState("");
  const requested = useRef("");
  const project = sessionId ? projects[sessionId] : undefined;
  const [unseen, setUnseen] = useUnseenArtifacts(sessionId, project, running);

  useEffect(
    () => () => {
      if (preview) URL.revokeObjectURL(preview.url);
    },
    [preview],
  );

  const load = useCallback(async () => {
    if (!sessionId) return;
    setArtifacts(null);
    setStatus("");
    try {
      const listed = await api.listArtifacts(sessionId, project);
      setFresh(new Set(unseenArtifacts(seenArtifacts(sessionId), listed).map((a) => a.name)));
      markArtifactsSeen(sessionId, listed);
      setUnseen(0);
      setArtifacts(listed);
    } catch (e) {
      setArtifacts([]);
      setStatus(String(e));
    }
  }, [sessionId, project, setUnseen]);

  const save = async (name: string) => {
    if (!sessionId) return;
    setStatus(`Saving ${name}...`);
    try {
      setStatus(`Saved to ${await api.saveArtifact(sessionId, project, name)}`);
    } catch (e) {
      setStatus(String(e));
    }
  };

  const show = async (name: string, type: string) => {
    if (!sessionId) return;
    requested.current = name;
    setStatus(`Loading ${name}...`);
    try {
      const bytes = await api.readArtifact(sessionId, project, name);
      if (requested.current !== name) return;
      setPreview({ name, type, url: URL.createObjectURL(new Blob([bytes], { type })) });
      setStatus("");
    } catch (e) {
      setStatus(String(e));
    }
  };

  if (!sessionId) return null;
  return (
    <>
      <button
        aria-label="Artifacts"
        title={unseen > 0 ? `Artifacts - ${unseen} new` : "Artifacts - files this chat produced"}
        className={cn(ICON_BUTTON, "relative", unseen > 0 && "bg-primary/15 text-primary")}
        onClick={() => {
          setOpen(true);
          load();
        }}
      >
        <Package size={16} aria-hidden="true" />
        {unseen > 0 && <span className="absolute top-0.5 right-0.5 size-2 rounded-full bg-primary" />}
      </button>
      {unseen > 0 && (
        <span className="sr-only">
          {unseen} new artifact{unseen === 1 ? "" : "s"}
        </span>
      )}
      <Dialog
        open={open}
        onOpenChange={(next) => {
          setOpen(next);
          if (!next) {
            requested.current = "";
            setPreview(null);
          }
        }}
      >
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-1">
              Artifacts
              <button aria-label="Reload artifacts" title="Reload artifacts" className={ICON_BUTTON} onClick={load}>
                <RefreshCw size={14} aria-hidden="true" />
              </button>
            </DialogTitle>
            <DialogDescription>Files this chat produced. Saving one puts it in Downloads.</DialogDescription>
          </DialogHeader>
          {preview?.type.startsWith("audio/") && <AudioPlayer src={preview.url} ariaLabel={preview.name} />}
          {preview?.type.startsWith("video/") && (
            <video
              controls
              src={preview.url}
              aria-label={`Video ${preview.name}`}
              className="max-h-[50vh] w-full rounded-md bg-black"
            />
          )}
          {preview?.type.startsWith("image/") && (
            <img src={preview.url} alt={preview.name} className="max-h-[50vh] w-full rounded-md object-contain" />
          )}
          {artifacts === null && <p className="text-muted-foreground">Loading...</p>}
          {artifacts?.length === 0 && !status && <p className="text-muted-foreground">No artifacts yet.</p>}
          <ul className="flex max-h-72 flex-col gap-1 overflow-y-auto">
            {artifacts?.map((artifact) => {
              const type = mediaType(artifact.name);
              const PreviewIcon = type?.startsWith("image/") ? Eye : Play;
              return (
                <li
                  key={artifact.name}
                  className={cn(
                    "flex items-center gap-2 rounded px-1",
                    preview?.name === artifact.name && "bg-secondary/60",
                  )}
                >
                  <span className="min-w-0 flex-1 truncate" title={artifact.name}>
                    {artifact.name}
                  </span>
                  {fresh.has(artifact.name) && (
                    <span className="shrink-0 rounded-full bg-primary/15 px-1.5 text-[0.65rem] font-semibold text-primary">
                      new
                    </span>
                  )}
                  <span className="shrink-0 text-[0.75rem] text-muted-foreground">
                    {formatSize(artifact.size)}
                    {artifact.modified_at && ` - ${new Date(artifact.modified_at).toLocaleString()}`}
                  </span>
                  {type && (
                    <button
                      aria-label={`Preview ${artifact.name}`}
                      title={`Preview ${artifact.name}`}
                      className={ICON_BUTTON}
                      onClick={() => show(artifact.name, type)}
                    >
                      <PreviewIcon size={14} aria-hidden="true" />
                    </button>
                  )}
                  <button
                    aria-label={`Save artifact ${artifact.name}`}
                    title={`Save ${artifact.name} to Downloads`}
                    className={ICON_BUTTON}
                    onClick={() => save(artifact.name)}
                  >
                    <Download size={14} aria-hidden="true" />
                  </button>
                </li>
              );
            })}
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
