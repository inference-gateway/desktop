import { Plus, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { WorkspaceVolume } from "@/lib/tauri";
import { EMPTY_VOLUME } from "@/lib/workspace-form";
import { advanceOnEnter, FIELD_CLASS, HINT_CLASS } from "./ClusterParts";

function VolumeRow({
  index,
  volume,
  onChange,
  onRemove,
}: {
  index: number;
  volume: WorkspaceVolume;
  onChange: (volume: WorkspaceVolume) => void;
  onRemove: () => void;
}) {
  const number = index + 1;
  return (
    <div className="flex items-center gap-2">
      <Input
        id={`workspace-volume-name-${index}`}
        aria-label={`Volume ${number} name`}
        autoFocus={!volume.name}
        value={volume.name}
        onChange={(e) => onChange({ ...volume, name: e.target.value })}
        onKeyDown={advanceOnEnter(`workspace-volume-claim-${index}`)}
        placeholder="footage"
      />
      <Input
        id={`workspace-volume-claim-${index}`}
        aria-label={`Volume ${number} claim`}
        value={volume.claim}
        onChange={(e) => onChange({ ...volume, claim: e.target.value })}
        onKeyDown={advanceOnEnter(`workspace-volume-mount-path-${index}`)}
        placeholder="raw-footage"
      />
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
          checked={volume.read_only}
          onChange={(e) => onChange({ ...volume, read_only: e.target.checked })}
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

/** The claims a workspace's computers mount: a name, the claim, where it mounts and whether it is read-only. */
export function WorkspaceVolumes({
  volumes,
  onChange,
}: {
  volumes: WorkspaceVolume[];
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
    </div>
  );
}
