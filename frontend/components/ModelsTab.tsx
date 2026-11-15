import { useCallback, useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Download, Loader2, Search, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api, Channel, type DownloadedModel, type HubFile, type HubModel } from "@/lib/tauri";
import { downloadArguments, pickedDownload } from "@/lib/hub-models";
import { fmtBytes } from "@/lib/timeline";

function ModelFiles({
  model,
  files,
  picked,
  downloading,
  onToggle,
  onDownload,
}: {
  model: HubModel;
  files: HubFile[];
  picked: ReadonlySet<string>;
  downloading: boolean;
  onToggle: (path: string) => void;
  onDownload: () => void;
}) {
  const download = pickedDownload(files, picked);
  return (
    <div className="mt-3 border-t border-border pt-3">
      <ul className="flex max-h-56 flex-col gap-1 overflow-y-auto">
        {files.map((file) => (
          <li key={file.path} className="flex items-center gap-2 text-[0.75rem]">
            <input
              type="checkbox"
              id={`hub-file-${model.id}-${file.path}`}
              checked={picked.has(file.path)}
              disabled={downloading}
              onChange={() => onToggle(file.path)}
            />
            <label htmlFor={`hub-file-${model.id}-${file.path}`} className="min-w-0 flex-1 truncate">
              {file.path}
            </label>
            <span className="text-muted-foreground">{fmtBytes(file.size)}</span>
          </li>
        ))}
      </ul>
      <div className="mt-3 flex items-center gap-2">
        <Button
          size="sm"
          aria-label={`Download ${model.id}`}
          disabled={downloading || download.paths.length === 0}
          onClick={onDownload}
        >
          {downloading ? <Loader2 size={14} className="animate-spin" /> : <Download size={14} />}
          {downloading ? "Downloading..." : "Download"}
        </Button>
        <span className="text-[0.75rem] text-muted-foreground">
          {download.paths.length} of {files.length} files, {fmtBytes(download.bytes)}
        </span>
      </div>
    </div>
  );
}

// Search the Hugging Face Hub and keep model files on this machine, backed by
// the hub model commands (backend/src/hub_models.rs) that shell out to
// `infer models`. The CLI owns the library in ~/.infer/models/huggingface.
export function ModelsTab() {
  const [found, setFound] = useState<HubModel[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [opened, setOpened] = useState<string | null>(null);
  const [files, setFiles] = useState<HubFile[]>([]);
  const [picked, setPicked] = useState<ReadonlySet<string>>(new Set());
  const [downloading, setDownloading] = useState<string | null>(null);
  const [progress, setProgress] = useState("");
  const [library, setLibrary] = useState<DownloadedModel[]>([]);
  const [error, setError] = useState("");
  const query = useRef<HTMLInputElement>(null);

  const refresh = useCallback(() => {
    api
      .listDownloadedModels()
      .then(setLibrary)
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(refresh, [refresh]);

  const search = async () => {
    const text = query.current?.value.trim() ?? "";
    if (!text) return;
    setSearching(true);
    setError("");
    setOpened(null);
    try {
      setFound(await api.searchHubModels(text));
    } catch (e) {
      setError(String(e));
    } finally {
      setSearching(false);
    }
  };

  const searchOnEnter = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") void search();
  };

  const open = async (id: string) => {
    if (opened === id) {
      setOpened(null);
      return;
    }
    setError("");
    try {
      const listed = await api.listHubModelFiles(id);
      setFiles(listed);
      setPicked(new Set(listed.map((file) => file.path)));
      setOpened(id);
    } catch (e) {
      setError(String(e));
    }
  };

  const toggle = (path: string) => {
    setPicked((before) => {
      const next = new Set(before);
      if (!next.delete(path)) next.add(path);
      return next;
    });
  };

  const download = async (id: string) => {
    setDownloading(id);
    setError("");
    setProgress("");
    const onLine = new Channel<string>();
    onLine.onmessage = setProgress;
    try {
      await api.downloadHubModel(id, downloadArguments(files, picked), onLine);
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setDownloading(null);
      setProgress("");
    }
  };

  const remove = async (id: string) => {
    setError("");
    try {
      await api.deleteDownloadedModel(id);
      refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">Models</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        Search the Hugging Face Hub and download model files to this computer, stored in
        ~/.infer/models/huggingface/&lt;owner&gt;/&lt;name&gt;. Only public models can be downloaded. Check a
        model&apos;s licence on its Hub page before you use it.
      </p>
      <div className="mb-4 flex items-center gap-2">
        <Input
          id="hub-search"
          ref={query}
          aria-label="Search Hugging Face"
          placeholder="Search Hugging Face, e.g. musetalk"
          className="h-7 w-72 text-[0.8rem]"
          onKeyDown={searchOnEnter}
        />
        <Button size="sm" disabled={searching} onClick={() => void search()}>
          {searching ? <Loader2 size={14} className="animate-spin" /> : <Search size={14} />}
          {searching ? "Searching..." : "Search models"}
        </Button>
        {error && (
          <span role="status" className="text-[0.75rem] text-err">
            {error}
          </span>
        )}
      </div>
      {downloading && (
        <p role="status" className="mb-4 text-[0.8rem] text-muted-foreground">
          {progress || `Downloading ${downloading}...`}
        </p>
      )}
      {found && found.length === 0 && <p className="mb-4 text-[0.8rem] text-muted-foreground">No models found.</p>}
      {found && found.length > 0 && (
        <div className="mb-6 flex flex-col gap-3">
          {found.map((model) => (
            <div key={model.id} className="rounded-lg border border-border bg-card p-3">
              <div className="flex items-center gap-2">
                <span className="min-w-0 truncate text-[0.8rem] font-medium">{model.id}</span>
                <span className="text-[0.75rem] text-muted-foreground">
                  {model.pipeline_tag && `${model.pipeline_tag}, `}
                  {model.downloads.toLocaleString()} downloads
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  aria-label={`Show files of ${model.id}`}
                  className="ml-auto"
                  onClick={() => void open(model.id)}
                >
                  {opened === model.id ? "Hide files" : "Files"}
                </Button>
              </div>
              {opened === model.id && (
                <ModelFiles
                  model={model}
                  files={files}
                  picked={picked}
                  downloading={downloading === model.id}
                  onToggle={toggle}
                  onDownload={() => void download(model.id)}
                />
              )}
            </div>
          ))}
        </div>
      )}
      <h3 className="mb-2 text-[0.9rem] font-semibold">Downloaded</h3>
      {library.length === 0 ? (
        <p className="text-[0.8rem] text-muted-foreground">No models downloaded yet.</p>
      ) : (
        <div className="flex flex-col gap-3">
          {library.map((model) => (
            <div key={model.id} className="rounded-lg border border-border bg-card p-3">
              <div className="flex items-center gap-2">
                <span className="text-[0.8rem] font-medium">{model.id}</span>
                <span className="text-[0.75rem] text-muted-foreground">
                  {model.files} files, {fmtBytes(model.bytes)}
                </span>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Delete model ${model.id}`}
                  title="Delete model"
                  className="ml-auto text-muted-foreground hover:text-destructive"
                  onClick={() => void remove(model.id)}
                >
                  <Trash2 size={14} />
                </Button>
              </div>
              <p className="mt-1 truncate text-[0.75rem] text-muted-foreground">{model.path}</p>
            </div>
          ))}
        </div>
      )}
    </>
  );
}
