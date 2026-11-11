import { Plus, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { folderOfClaim, onFolder, onFolderClaim } from "@/lib/cluster-folders";
import type { ClusterFolder, WorkspaceVolume } from "@/lib/tauri";
import { EMPTY_VOLUME } from "@/lib/workspace-form";
import { advanceOnEnter, FIELD_CLASS, HINT_CLASS } from "./ClusterParts";

const SELECT_CLASS = "rounded-md border border-border-strong bg-card px-2.5 py-1.5 text-[0.85rem] text-foreground";

function VolumeRow({
  index,
  volume,
  folders,
  onChange,
  onRemove,
}: {
  index: number;
  volume: WorkspaceVolume;
  folders: ClusterFolder[];
  onChange: (volume: WorkspaceVolume) => void;
  onRemove: () => void;
}) {
  const number = index + 1;
  const folder = folders.find((each) => each.name === folderOfClaim(volume.claim.trim()));
  const locked = !!folder?.read_only;
  const mapsFolders = folders.length > 0;
  const onSource = (name: string) => {
    const chosen = folders.find((each) => each.name === name);
    onChange(chosen ? onFolder(volume, chosen) : { ...volume, claim: "" });
  };
  const afterName = mapsFolders ? `workspace-volume-source-${index}` : `workspace-volume-claim-${index}`;
  return (
    <div className="flex items-center gap-2">
      <Input
        id={`workspace-volume-name-${index}`}
        aria-label={`Volume ${number} name`}
        autoFocus={!volume.name}
        value={volume.name}
        onChange={(e) => onChange({ ...volume, name: e.target.value })}
        onKeyDown={advanceOnEnter(afterName)}
        placeholder="footage"
      />
      {mapsFolders && (
        <select
          id={`workspace-volume-source-${index}`}
          aria-label={`Volume ${number} source`}
          value={folder?.name ?? ""}
          onChange={(e) => onSource(e.target.value)}
          className={SELECT_CLASS}
        >
          <option value="">An existing claim</option>
          {folders.map((each) => (
            <option key={each.name} value={each.name}>
              {each.path}
            </option>
          ))}
        </select>
      )}
      {!folder && (
        <Input
          id={`workspace-volume-claim-${index}`}
          aria-label={`Volume ${number} claim`}
          value={volume.claim}
          onChange={(e) => onChange({ ...volume, claim: e.target.value })}
          onKeyDown={advanceOnEnter(`workspace-volume-mount-path-${index}`)}
          placeholder="raw-footage"
        />
      )}
      <span aria-hidden className="shrink-0 text-muted-foreground">
        &rarr;
      </span>
      <Input
        id={`workspace-volume-mount-path-${index}`}
        aria-label={`Volume ${number} mount path`}
        value={volume.mount_path}
        onChange={(e) => onChange({ ...volume, mount_path: e.target.value })}
        placeholder="/mnt/footage"
      />
      <label className="flex shrink-0 items-center gap-1.5 text-[0.8rem]">
        <input
          type="checkbox"
          id={`workspace-volume-read-only-${index}`}
          aria-label={`Volume ${number} read-only`}
          checked={volume.read_only || locked}
          disabled={locked}
          title={locked ? "This folder is read-only for every workspace, set under Settings > Compute" : undefined}
          onChange={(e) => onChange(onFolderClaim({ ...volume, read_only: e.target.checked }))}
          className="h-4 w-4 accent-primary"
        />
        Read-only
      </label>
      <Button variant="outline" size="xs" aria-label={`Remove volume ${number}`} onClick={onRemove}>
        <X size={12} />
      </Button>
    </div>
  );
}

/** What a workspace's computers mount: a name, a claim or a folder of this computer, where it mounts and whether it
 * is read-only. */
export function WorkspaceVolumes({
  volumes,
  folders,
  onChange,
}: {
  volumes: WorkspaceVolume[];
  folders: ClusterFolder[];
  onChange: (volumes: WorkspaceVolume[]) => void;
}) {
  return (
    <div className={FIELD_CLASS}>
      <span className="text-[0.8rem] font-medium">Volumes</span>
      {volumes.map((volume, index) => (
        <VolumeRow
          key={index}
          index={index}
          volume={volume}
          folders={folders}
          onChange={(changed) => onChange(volumes.map((each, at) => (at === index ? changed : each)))}
          onRemove={() => onChange(volumes.filter((_, at) => at !== index))}
        />
      ))}
      <div>
        <Button variant="outline" size="xs" onClick={() => onChange([...volumes, EMPTY_VOLUME])}>
          <Plus size={12} />
          Add volume
        </Button>
      </div>
      <span className={HINT_CLASS}>
        Each mounts a PersistentVolumeClaim that already exists in the workspace's namespace - an app installed once, or
        content its computers share. Read-only is for originals the agent copies before it edits them. Volumes are fixed
        when a computer is created, so a change reaches the chats started after it.
      </span>
      {folders.length > 0 && (
        <span className={HINT_CLASS}>
          On the local cluster a volume can map a folder of this computer, picked by its path, to the mount path. The
          folder shows up one level below it, as &lt;mount path&gt;/&lt;folder name&gt;. Folders are added under
          Settings &gt; Compute.
        </span>
      )}
    </div>
  );
}
