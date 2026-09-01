import { api } from "./tauri";

export const DEFAULT_REGISTRY_URL = "https://cdn.jsdelivr.net/gh/inference-gateway/skills@main/catalog.json";
const LEGACY_REGISTRY_URL = "https://registry.inference-gateway.com/skills/";
const REGISTRY_KEY = "skillsRegistryUrl";

export type SkillMetadata = {
  name: string;
  description: string;
  version: string;
};

export type SkillsCatalog = {
  version: number;
  skills: SkillMetadata[];
};

export function getRegistryUrl(): string {
  const stored = localStorage.getItem(REGISTRY_KEY);
  if (!stored || stored === LEGACY_REGISTRY_URL) return DEFAULT_REGISTRY_URL;
  return stored;
}

export function setRegistryUrl(url: string): void {
  localStorage.setItem(REGISTRY_KEY, url);
  try {
    api.saveSkillsRegistryUrl(url).catch(() => {});
  } catch {
    /* outside Tauri (tests): skip */
  }
}

/** Adopt a registry URL imported from another machine when localStorage has
    not populated one yet (called once at startup). */
export function hydrateRegistry(url: string): void {
  if (url && localStorage.getItem(REGISTRY_KEY) === null) setRegistryUrl(url);
}

export async function fetchSkillsCatalog(url?: string): Promise<SkillsCatalog> {
  const res = await fetch(url || getRegistryUrl());
  if (!res.ok) throw new Error(`skills catalog ${res.status}`);
  return res.json();
}
