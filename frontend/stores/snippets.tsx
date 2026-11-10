import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { api } from "@/lib/tauri";
import { hydrateRegistry } from "@/lib/skills";
import {
  loadSnippets,
  saveSnippets,
  defaultForId,
  DEFAULT_SNIPPETS,
  mergeSnippets,
  type Snippet,
} from "@/lib/snippets";

function useSnippetsStore() {
  const [snippets, setSnippets] = useState<Snippet[]>(() => loadSnippets());

  const resetSnippet = useCallback((id: string) => {
    const def = defaultForId(id);
    if (!def) return;
    setSnippets((prev) => {
      const next = prev.map((s) => (s.id === id ? { ...def } : s));
      saveSnippets(next);
      return next;
    });
  }, []);

  const updateSnippet = useCallback((id: string, patch: { label?: string; prompt?: string }) => {
    setSnippets((prev) => {
      const next = prev.map((s) => (s.id === id ? { ...s, ...patch } : s));
      saveSnippets(next);
      return next;
    });
  }, []);

  const resetAllSnippets = useCallback(() => {
    setSnippets(DEFAULT_SNIPPETS);
    saveSnippets(DEFAULT_SNIPPETS);
  }, []);

  const reloadDesktopData = useCallback(() => {
    return api.readDesktopData().then((data) => {
      if (data.snippets.length > 0) {
        const next = mergeSnippets(data.snippets);
        setSnippets(next);
        saveSnippets(next);
      }
      hydrateRegistry(data.skills_registry_url);
      return data;
    });
  }, []);

  useEffect(() => {
    void reloadDesktopData().catch(() => {});
  }, [reloadDesktopData]);

  return useMemo(
    () => ({ snippets, resetSnippet, updateSnippet, resetAllSnippets, reloadDesktopData }),
    [snippets, resetSnippet, updateSnippet, resetAllSnippets, reloadDesktopData],
  );
}

type SnippetsStore = ReturnType<typeof useSnippetsStore>;

const SnippetsContext = createContext<SnippetsStore | null>(null);

export function SnippetsProvider({ children }: { children: ReactNode }) {
  return <SnippetsContext.Provider value={useSnippetsStore()}>{children}</SnippetsContext.Provider>;
}

/** The composer's prompt snippets, and the desktop data file they are saved in. */
export function useSnippets(): SnippetsStore {
  const ctx = useContext(SnippetsContext);
  if (!ctx) throw new Error("useSnippets must be used within SnippetsProvider");
  return ctx;
}
