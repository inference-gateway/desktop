import type { HubFile } from "@/lib/tauri";

/** The files a download fetches and what they weigh: the picked ones, of those a repository lists. */
export function pickedDownload(files: HubFile[], picked: ReadonlySet<string>): { paths: string[]; bytes: number } {
  const chosen = files.filter((file) => picked.has(file.path));
  return { paths: chosen.map((file) => file.path), bytes: chosen.reduce((total, file) => total + file.size, 0) };
}

/**
 * The file names to hand the CLI: none when every file is picked, since that
 * is how it is told to fetch the whole repository.
 */
export function downloadArguments(files: HubFile[], picked: ReadonlySet<string>): string[] {
  const { paths } = pickedDownload(files, picked);
  return paths.length === files.length ? [] : paths;
}
