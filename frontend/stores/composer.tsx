import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type ComposerSettings } from "@/lib/tauri";
import { BACKEND_EVENTS } from "@/lib/events";
import { useBackend } from "@/stores/backend";
import { useStatusActions } from "@/stores/status";

function useComposerStore() {
  const { ready } = useBackend();
  const { setError } = useStatusActions();
  const [composer, setComposer] = useState<ComposerSettings | null>(null);
  const loadedOnce = useRef(false);

  const reload = useCallback(async () => {
    try {
      setComposer(await api.getComposer());
    } catch (e) {
      setComposer(null);
      setError(`Composer config: ${e}`);
    }
  }, [setError]);

  useEffect(() => {
    if (!ready || loadedOnce.current) return;
    loadedOnce.current = true;
    void reload();
  }, [ready, reload]);

  useEffect(() => {
    const unlisten = listen(BACKEND_EVENTS.composerChanged, () => void reload());
    return () => {
      unlisten.then((f) => f());
    };
  }, [reload]);

  return useMemo(() => ({ composer, reload }), [composer, reload]);
}

type ComposerStore = ReturnType<typeof useComposerStore>;

const ComposerContext = createContext<ComposerStore | null>(null);

export function ComposerProvider({ children }: { children: ReactNode }) {
  return <ComposerContext.Provider value={useComposerStore()}>{children}</ComposerContext.Provider>;
}

/** The composer config the CLI resolves from ~/.infer/composer.yaml, loaded
 * once the backend is ready and reloaded whenever the file changes, plus the
 * reload itself. Not loaded means the defaults apply. */
export function useComposer(): ComposerStore {
  const ctx = useContext(ComposerContext);
  if (!ctx) throw new Error("useComposer must be used within ComposerProvider");
  return ctx;
}
