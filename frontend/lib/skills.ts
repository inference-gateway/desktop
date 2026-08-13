// Skills registry: fetch & cache the remote skills catalog, track locally-configured skills.
// ponytail: localStorage-based config - migrate to DesktopConfig when backend persistence matters.

const DEFAULT_REGISTRY_URL = "https://registry.inference-gateway.com/skills/";
const REGISTRY_KEY = "skillsRegistryUrl";
const CONFIGURED_KEY = "configuredSkills";

export type SkillMetadata = {
  name: string;
  description: string;
  version: string;
};

export type SkillsCatalog = {
  version: string;
  skills: SkillMetadata[];
};

export function getRegistryUrl(): string {
  return localStorage.getItem(REGISTRY_KEY) || DEFAULT_REGISTRY_URL;
}

export function setRegistryUrl(url: string): void {
  localStorage.setItem(REGISTRY_KEY, url);
}

export function getConfiguredSkills(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(CONFIGURED_KEY) || "[]"));
  } catch {
    return new Set();
  }
}

export function setConfiguredSkill(name: string, configured: boolean): void {
  const set = getConfiguredSkills();
  if (configured) set.add(name);
  else set.delete(name);
  localStorage.setItem(CONFIGURED_KEY, JSON.stringify([...set]));
}

export async function fetchSkillsCatalog(url?: string): Promise<SkillsCatalog> {
  const res = await fetch(url || getRegistryUrl());
  if (!res.ok) throw new Error(`skills catalog ${res.status}`);
  return res.json();
}
