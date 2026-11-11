import { useEffect, useState } from "react";
import { Plus, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { EMPTY_FOLDER, foldersProblem } from "@/lib/cluster-folders";
import { api, type ClusterFolder } from "@/lib/tauri";
import { advanceOnEnter, HINT_CLASS, Problem } from "./ClusterParts";

function FolderRow({
  index,
  folder,
  onChange,
  onRemove,
}: {
  index: number;
  folder: ClusterFolder;
  onChange: (folder: ClusterFolder) => void;
  onRemove: () => void;
}) {
  const number = index + 1;
  return (
    <div className="flex items-center gap-2">
      <Input
        id={`cluster-folder-name-${index}`}
        aria-label={`Folder ${number} name`}
        autoFocus={!folder.name}
        value={folder.name}
        onChange={(e) => onChange({ ...folder, name: e.target.value })}
        onKeyDown={advanceOnEnter(`cluster-folder-path-${index}`)}
        placeholder="footage"
        className="max-w-40"
      />
      <Input
        id={`cluster-folder-path-${index}`}
        aria-label={`Folder ${number} path`}
        value={folder.path}
        onChange={(e) => onChange({ ...folder, path: e.target.value })}
        placeholder="/Users/you/Movies/raw"
      />
      <label className="flex shrink-0 items-center gap-1.5 text-[0.8rem]">
        <input
          type="checkbox"
          id={`cluster-folder-read-only-${index}`}
          aria-label={`Folder ${number} read-only`}
          checked={folder.read_only}
          onChange={(e) => onChange({ ...folder, read_only: e.target.checked })}
          className="h-4 w-4 accent-primary"
        />
        Read-only
      </label>
      <Button variant="outline" size="xs" aria-label={`Remove folder ${number}`} onClick={onRemove}>
        <X size={12} />
      </Button>
    </div>
  );
}

/** The folders of this computer the local cluster shares with its workspaces. They and the cluster's runtime reach the
 * cluster only when it is created, so while a change is `pending` this asks for the cluster to be recreated, which
 * `onRecreate` does. */
export function ClusterFolders({
  busy,
  pending,
  onSaved,
  onRecreate,
}: {
  busy: boolean;
  pending: boolean;
  onSaved: () => void;
  onRecreate: () => Promise<void>;
}) {
  const [folders, setFolders] = useState<ClusterFolder[]>([]);
  const [confirming, setConfirming] = useState(false);
  const [problem, setProblem] = useState("");
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    api
      .clusterFolders()
      .then(setFolders)
      .catch((e) => setProblem(String(e)));
  }, []);

  const change = (next: ClusterFolder[]) => {
    setFolders(next);
    setSaved(false);
  };

  const save = async () => {
    const blocked = foldersProblem(folders);
    if (blocked) return setProblem(blocked);
    try {
      await api.saveClusterFolders(folders);
      setProblem("");
      setSaved(true);
      onSaved();
    } catch (e) {
      setProblem(String(e));
    }
  };

  const recreate = async () => {
    setConfirming(false);
    await onRecreate();
  };

  return (
    <section className="mt-6 flex flex-col gap-2">
      <h3 className="text-[0.9rem] font-semibold">Local cluster folders</h3>
      <p className={HINT_CLASS}>
        Folders of this computer the local cluster shares with its workspaces, mounted straight from here with no copy.
        A workspace mounts one as a volume, on the claim folder-&lt;name&gt;. A read-only folder cannot be changed from
        any workspace - a writable one can still be mounted read-only by a single workspace.
      </p>
      <Problem text={problem} />
      {folders.map((folder, index) => (
        <FolderRow
          key={index}
          index={index}
          folder={folder}
          onChange={(changed) => change(folders.map((each, at) => (at === index ? changed : each)))}
          onRemove={() => change(folders.filter((_, at) => at !== index))}
        />
      ))}
      <div className="flex items-center gap-2">
        <Button variant="outline" size="xs" onClick={() => change([...folders, EMPTY_FOLDER])}>
          <Plus size={12} />
          Add folder
        </Button>
        <Button size="xs" onClick={save}>
          Save folders
        </Button>
        {saved && (
          <span aria-live="polite" className={HINT_CLASS}>
            Folders saved.
          </span>
        )}
      </div>
      {pending && (
        <div
          role="alert"
          className="flex flex-col gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 px-4 py-2.5 text-[0.8rem]"
        >
          <span>
            The local cluster runs with other folders than these, or on another runtime than the one chosen. Both reach
            a cluster only as it is created, so recreating it is what applies them. That deletes the cluster's machines
            and everything on them.
          </span>
          <div className="flex gap-2">
            {confirming ? (
              <>
                <Button variant="destructive" size="xs" disabled={busy} onClick={recreate}>
                  Delete its machines and recreate
                </Button>
                <Button variant="outline" size="xs" onClick={() => setConfirming(false)}>
                  Keep the cluster
                </Button>
              </>
            ) : (
              <Button variant="outline" size="xs" disabled={busy} onClick={() => setConfirming(true)}>
                Recreate local cluster
              </Button>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
