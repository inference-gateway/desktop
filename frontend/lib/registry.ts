const CATALOG_URL = "https://cdn.jsdelivr.net/gh/inference-gateway/agents@main/catalog.json";

export type CatalogAgent = { name: string; description: string; version: string; skills: string[] };

type RawCatalog = {
  agents?: {
    metadata?: { name?: string; description?: string; version?: string };
    spec?: { skills?: { name?: string }[] };
  }[];
};

export function toCatalogAgents(data: RawCatalog | null | undefined): CatalogAgent[] {
  return (data?.agents ?? []).map((a) => ({
    name: a?.metadata?.name ?? "",
    description: a?.metadata?.description ?? "",
    version: a?.metadata?.version ?? "",
    skills: (a?.spec?.skills ?? []).map((s) => s?.name ?? "").filter(Boolean),
  }));
}

export async function fetchAgentCatalog(): Promise<CatalogAgent[]> {
  const res = await fetch(CATALOG_URL);
  if (!res.ok) throw new Error(`catalog ${res.status}`);
  return toCatalogAgents(await res.json());
}
