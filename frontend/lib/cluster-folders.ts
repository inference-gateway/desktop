import type { ClusterFolder, WorkspaceVolume } from "./tauri";

export const EMPTY_FOLDER: ClusterFolder = { name: "", path: "", read_only: false };

const CLAIM_PREFIX = "folder-";
const READ_ONLY_SUFFIX = "-ro";
const FOLDER_NAME = /^[a-z0-9]([-a-z0-9]*[a-z0-9])?$/;
const FOLDER_NAME_MAX = 40;

/** The claim the local cluster gives a folder: one to write to it and one, ending in `-ro`, that cannot. */
export function folderClaim(name: string, readOnly: boolean): string {
  return `${CLAIM_PREFIX}${name}${readOnly ? READ_ONLY_SUFFIX : ""}`;
}

/** The folder a claim belongs to, or null for a claim that is not one of the local cluster's folders. */
export function folderOfClaim(claim: string): string | null {
  if (!claim.startsWith(CLAIM_PREFIX)) return null;
  const name = claim.slice(CLAIM_PREFIX.length);
  return name.endsWith(READ_ONLY_SUFFIX) ? name.slice(0, -READ_ONLY_SUFFIX.length) : name;
}

/** The volume on the claim its read-only switch asks for. A folder mounts through its `-ro` claim when read-only,
 * because that is what stops a write to it. Any other claim stays as written. */
export function onFolderClaim(volume: WorkspaceVolume): WorkspaceVolume {
  const folder = folderOfClaim(volume.claim.trim());
  return folder ? { ...volume, claim: folderClaim(folder, volume.read_only) } : volume;
}

/** The volume mounting `folder`: on its claim, read-only when the folder is, and named after it unless it already
 * has a name. */
export function onFolder(volume: WorkspaceVolume, folder: ClusterFolder): WorkspaceVolume {
  const read_only = volume.read_only || folder.read_only;
  return { ...volume, name: volume.name.trim() || folder.name, claim: folderClaim(folder.name, read_only), read_only };
}

/** Why the folders cannot be saved yet, or null when they can. Whether a path exists is checked on saving. */
export function foldersProblem(folders: ClusterFolder[]): string | null {
  const names = folders.map((folder) => folder.name.trim());
  for (const [index, folder] of folders.entries()) {
    const name = names[index];
    if (!name) return `Give folder ${index + 1} a name.`;
    if (name.length > FOLDER_NAME_MAX || !FOLDER_NAME.test(name) || name.endsWith(READ_ONLY_SUFFIX))
      return `Name folder ${index + 1} with lowercase letters, digits or dashes, not ending in ${READ_ONLY_SUFFIX}.`;
    if (names.indexOf(name) !== index) return `Two folders are named ${name} - give each its own name.`;
    if (!folder.path.trim().startsWith("/")) return `Give folder ${name} a path on this computer that starts with /.`;
  }
  return null;
}
