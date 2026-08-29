// Skills registry: fetch the remote skills catalog. Installed state lives on
// disk (~/.infer/skills) and is read via api.listInstalledSkills().

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
}

export async function fetchSkillsCatalog(url?: string): Promise<SkillsCatalog> {
  const res = await fetch(url || getRegistryUrl());
  if (!res.ok) throw new Error(`skills catalog ${res.status}`);
  return res.json();
}
