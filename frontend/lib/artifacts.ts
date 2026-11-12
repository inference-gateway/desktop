import type { Artifact } from "./tauri";

const MEDIA_TYPES: Record<string, string> = {
  mp3: "audio/mpeg",
  wav: "audio/wav",
  m4a: "audio/mp4",
  aac: "audio/aac",
  ogg: "audio/ogg",
  flac: "audio/flac",
  mp4: "video/mp4",
  m4v: "video/mp4",
  mov: "video/quicktime",
  webm: "video/webm",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
};

/** The media type of an artifact the app can play or show, by its extension. */
export const mediaType = (name: string): string | undefined =>
  MEDIA_TYPES[name.slice(name.lastIndexOf(".") + 1).toLowerCase()];

/** Identifies one version of an artifact, so a rewritten file counts as new again. */
export const artifactKey = (a: Artifact) => `${a.name}|${a.size}|${a.modified_at}`;

/** The artifacts that were added or changed since the `seen` versions. */
export const unseenArtifacts = (seen: ReadonlySet<string>, listed: Artifact[]) =>
  listed.filter((a) => !seen.has(artifactKey(a)));

const seenKey = (sessionId: string) => `artifacts-seen:${sessionId}`;

/** The artifact versions the user last saw in a chat's Artifacts dialog. */
export function seenArtifacts(sessionId: string): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(seenKey(sessionId)) ?? "[]"));
  } catch {
    return new Set();
  }
}

export function markArtifactsSeen(sessionId: string, listed: Artifact[]) {
  localStorage.setItem(seenKey(sessionId), JSON.stringify(listed.map(artifactKey)));
}
