import type { UpdateInfo } from "./tauri";

/** Release tags are spelled `v0.63.1` while installed versions spell `0.63.1`.
 * Only a leading `v` ahead of a digit is stripped, so anything else passes through. */
export function stripV(version: string): string {
  return version.trim().replace(/^v(?=\d)/, "");
}

/** The versions a component's row offers: the newest 5 tags, nothing on a `dev`
 * or locally built binary, and nothing while the release list is unknown. */
export function selectableVersions(update: UpdateInfo): string[] {
  if (update.current === "dev" || update.current === "") return [];
  return (update.versions ?? []).filter(Boolean).slice(0, 5);
}

/** Numeric components of a version, or null when it is not one (e.g. `dev`). */
function versionParts(version: string): number[] | null {
  const parts = stripV(version).split(".").map(Number);
  return parts.length > 0 && parts.every(Number.isFinite) ? parts : null;
}

/** What installing `target` does to `installed`: newer means "Upgrade", older
 * "Downgrade", and picking the installed version is no action at all. Compared
 * numerically, not as strings, so `0.9.0` stays older than `0.10.0`. */
export function versionAction(installed: string, target: string): "Upgrade" | "Downgrade" | null {
  const current = versionParts(installed);
  const next = versionParts(target);
  if (!current || !next) return null;
  for (let i = 0; i < Math.max(current.length, next.length); i++) {
    if ((next[i] ?? 0) > (current[i] ?? 0)) return "Upgrade";
    if ((next[i] ?? 0) < (current[i] ?? 0)) return "Downgrade";
  }
  return null;
}
