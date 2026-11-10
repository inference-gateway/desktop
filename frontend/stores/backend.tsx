import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MutableRefObject,
  type ReactNode,
} from "react";
import {
  api,
  Channel,
  type A2aStatus,
  type McpStatus,
  type ProgressEvent,
  type Shortcut,
  type UpdateInfo,
} from "@/lib/tauri";
import { markApplied, stripV, updateButtonState } from "@/lib/updates";
import { useStatusActions } from "@/stores/status";

export const MODEL_STORAGE_KEY = "selectedModel";
const UPDATE_CACHE_KEY = "updateCheck";
const UPDATE_INTERVAL_MS = 6 * 60 * 60 * 1000;
const MAX_RETRIES = 10;

/** Status text for an install progress event, shared by the full init flow and
 * per-component installs; "Ready" and "Error" carry extra side-effects there. */
function progressStatus(event: ProgressEvent, what = "infer"): string {
  switch (event.kind) {
    case "Checking":
      return "Checking for infer binary...";
    case "Downloading":
      return event.total > 0
        ? `Downloading ${what}... ${Math.round((event.received / event.total) * 100)}%`
        : `Downloading ${what}...`;
    case "Verifying":
      return "Verifying download...";
    case "Installing":
      return `Installing ${what}...`;
    case "Initializing":
      return "Running initial setup...";
    case "Ready":
      return "Ready";
    case "Error":
      return `Error: ${event.message}`;
  }
}

function useBackendStore() {
  const { setStatus, setError } = useStatusActions();
  const [ready, setReady] = useState(false);
  const [models, setModels] = useState<string[]>([]);
  const [model, setModelState] = useState<string>(() => localStorage.getItem(MODEL_STORAGE_KEY) || "");
  const [updates, setUpdates] = useState<UpdateInfo[]>([]);
  const [updating, setUpdating] = useState(false);
  const updatingRef = useRef(false);
  const updateCheckAfterInit = useRef<Promise<boolean | null>>(Promise.resolve(null));
  const [tools, setTools] = useState<string[]>([]);
  const [shortcuts, setShortcuts] = useState<Shortcut[]>([]);
  const [mcpStatus, setMcpStatus] = useState<McpStatus | null>(null);
  const [a2aStatus, setA2aStatus] = useState<A2aStatus | null>(null);
  const [servicesStarting, setServicesStarting] = useState(false);
  const initRan = useRef(false);
  const sessionStopper: MutableRefObject<() => Promise<void>> = useRef(async () => {});

  const refreshStatus = useCallback(() => {
    api
      .mcpStatus()
      .then(setMcpStatus)
      .catch(() => {});
    api
      .a2aStatus()
      .then(setA2aStatus)
      .catch(() => {});
  }, []);

  const startServices = useCallback(() => {
    setServicesStarting(true);
    api
      .startServices()
      .finally(() => setServicesStarting(false))
      .finally(refreshStatus);
  }, [refreshStatus]);

  const setModel = useCallback((m: string) => {
    setModelState(m);
    localStorage.setItem(MODEL_STORAGE_KEY, m);
    api.setDefaultModel(m).catch(() => {});
  }, []);

  const populateModels = useCallback((list: string[]) => {
    setModels(list);
    setModelState((cur) => {
      if (cur && list.includes(cur)) return cur;
      const saved = localStorage.getItem(MODEL_STORAGE_KEY);
      if (saved && list.includes(saved)) return saved;
      return list[0] ?? "";
    });
  }, []);

  const fetchModelsWithRetry = useCallback(
    async (attempt = 0) => {
      try {
        populateModels(await api.listModels());
      } catch {
        if (attempt < MAX_RETRIES) setTimeout(() => fetchModelsWithRetry(attempt + 1), 1500);
      }
    },
    [populateModels],
  );

  const startGatewayThenModels = useCallback(
    async (force = false, restart = false, tag: string | null = null) => {
      setStatus("Starting gateway...");
      try {
        await api.startGateway(force, restart, tag);
      } catch (e) {
        console.error("start_gateway failed:", e);
        setError(String(e));
      }
      setStatus("Ready");
      fetchModelsWithRetry();
    },
    [setStatus, setError, fetchModelsWithRetry],
  );

  const checkForUpdates = useCallback(async (force = false) => {
    const cached = JSON.parse(localStorage.getItem(UPDATE_CACHE_KEY) || "null");
    if (!force && cached && Date.now() - cached.checkedAt < UPDATE_INTERVAL_MS) {
      setUpdates(cached.updates);
      return true;
    }
    try {
      const list = await api.checkUpdates();
      localStorage.setItem(UPDATE_CACHE_KEY, JSON.stringify({ checkedAt: Date.now(), updates: list }));
      setUpdates(list);
      return true;
    } catch (e) {
      console.error("check_updates failed:", e);
      if (cached) setUpdates(cached.updates);
      return false;
    }
  }, []);

  const initBackend = useCallback(
    async (force = false, restart = false) => {
      let settle: (checked: boolean | null) => void = () => {};
      updateCheckAfterInit.current = new Promise((resolve) => (settle = resolve));
      try {
        const ch = new Channel<ProgressEvent>();
        ch.onmessage = (event) => {
          switch (event.kind) {
            case "Ready":
              setStatus("Ready");
              setReady(true);
              startGatewayThenModels(force, restart)
                .then(() => checkForUpdates(true))
                .then(settle, () => settle(null));
              api
                .listTools()
                .then(setTools)
                .catch(() => {});
              api
                .listShortcuts()
                .then(setShortcuts)
                .catch(() => {});
              refreshStatus();
              startServices();
              break;
            case "Error":
              setError(progressStatus(event));
              break;
            default:
              setStatus(progressStatus(event));
          }
        };
        await api.checkAndInstallCli(ch, force, restart);
      } catch (err) {
        setError(`Setup failed: ${err}`);
        settle(null);
      }
    },
    [setStatus, setError, startGatewayThenModels, checkForUpdates, refreshStatus, startServices],
  );

  const restartBackend = useCallback(
    async (force: boolean) => {
      await sessionStopper.current();
      setStatus(force ? "Updating..." : "Restarting CLI...");
      setReady(false);
      await initBackend(force, true);
    },
    [setStatus, initBackend],
  );

  useEffect(() => {
    if (initRan.current) return;
    initRan.current = true;

    const staleRaw = localStorage.getItem(UPDATE_CACHE_KEY);
    if (staleRaw) {
      try {
        const stale = JSON.parse(staleRaw);
        localStorage.setItem(UPDATE_CACHE_KEY, JSON.stringify({ ...stale, checkedAt: 0 }));
      } catch {
        localStorage.removeItem(UPDATE_CACHE_KEY);
      }
    }
    initBackend();
    const t = setInterval(() => checkForUpdates(true), UPDATE_INTERVAL_MS);
    return () => clearInterval(t);
  }, [initBackend, checkForUpdates]);

  const installDesktop = useCallback(
    (tag: string | null = null) => {
      setStatus("Updating app...");
      const ch = new Channel<ProgressEvent>();
      ch.onmessage = (event) => setStatus(progressStatus(event, "app update"));
      return api.installDesktopUpdate(ch, tag);
    },
    [setStatus],
  );

  // One apply at a time, and busy until the follow-up check has answered, so
  // neither update button turns clickable again on rows that are about to change.
  const applyUpdates = useCallback(async () => {
    if (updatingRef.current) return;
    updatingRef.current = true;
    setUpdating(true);
    try {
      const desktopOutdated = updates.some((u) => u.outdated && u.name === "Desktop");
      localStorage.removeItem(UPDATE_CACHE_KEY);
      await restartBackend(true);
      if ((await updateCheckAfterInit.current) === false) setUpdates(markApplied);
      if (desktopOutdated) await installDesktop();
    } catch (err) {
      setError(`App update failed: ${err}`);
    } finally {
      updatingRef.current = false;
      setUpdating(false);
    }
  }, [restartBackend, installDesktop, updates, setError]);

  // Per-component version pinning: the tag picked in the Updates tab, per
  // component row, kept until it has been installed.
  const [selectedVersions, setSelectedVersions] = useState<Record<string, string>>({});
  const selectVersion = useCallback((name: string, tag: string | null) => {
    setSelectedVersions((prev) => {
      const next = { ...prev };
      if (tag === null) delete next[name];
      else next[name] = tag;
      return next;
    });
  }, []);

  // Installs exactly one component at the version picked for it, and leaves
  // the other components alone. Without a pick the one-click flow applies.
  const installUpdate = useCallback(
    async (name: string) => {
      const tag = selectedVersions[name] ?? null;
      if (!tag) return;
      try {
        if (name === "Desktop") {
          // The updater relaunches the app, so this never resolves.
          await installDesktop(tag);
        } else if (name === "Gateway") {
          await sessionStopper.current();
          setReady(false);
          await startGatewayThenModels(true, true, tag);
          setReady(true);
        } else {
          // Only the CLI binary is replaced and its daemon restarted; the
          // running gateway keeps serving, so nothing else is rebuilt.
          await sessionStopper.current();
          setReady(false);
          const ch = new Channel<ProgressEvent>();
          ch.onmessage = (event) => setStatus(progressStatus(event));
          await api.checkAndInstallCli(ch, true, true, tag);
          setReady(true);
          startGatewayThenModels(false, false);
        }
        selectVersion(name, null);
        await checkForUpdates(true);
      } catch (err) {
        setError(`Failed to install ${name} ${stripV(tag)}: ${err}`);
      }
    },
    [selectedVersions, installDesktop, startGatewayThenModels, selectVersion, checkForUpdates, setStatus, setError],
  );

  const outdated = updates.filter((u) => u.outdated);
  const versionBadge = updates.map((u) => `${u.name} ${u.current}`).join(" · ");
  const updateBannerText = outdated.length
    ? `${outdated.map((u) => `${u.name} ${u.latest}`).join(", ")} available - restart to update`
    : "";
  const updateButton = updateButtonState(updates, updating);

  return useMemo(
    () => ({
      ready,
      models,
      model,
      setModel,
      tools,
      shortcuts,
      mcpStatus,
      a2aStatus,
      refreshStatus,
      servicesStarting,
      startServices,
      startGatewayThenModels,
      restartBackend,
      sessionStopper,
      updates,
      selectedVersions,
      selectVersion,
      installUpdate,
      versionBadge,
      updateBannerText,
      updateButton,
      checkForUpdates,
      applyUpdates,
    }),
    [
      ready,
      models,
      model,
      setModel,
      tools,
      shortcuts,
      mcpStatus,
      a2aStatus,
      refreshStatus,
      servicesStarting,
      startServices,
      startGatewayThenModels,
      restartBackend,
      updates,
      selectedVersions,
      selectVersion,
      installUpdate,
      versionBadge,
      updateBannerText,
      updateButton,
      checkForUpdates,
      applyUpdates,
    ],
  );
}

type BackendStore = ReturnType<typeof useBackendStore>;

const BackendContext = createContext<BackendStore | null>(null);

export function BackendProvider({ children }: { children: ReactNode }) {
  return <BackendContext.Provider value={useBackendStore()}>{children}</BackendContext.Provider>;
}

/** The CLI, gateway and daemon the app runs on: whether they are ready, the
 * models and tools they offer, and installing updates to them. `sessionStopper`
 * is set by whoever runs sessions, and is awaited before a restart. */
export function useBackend(): BackendStore {
  const ctx = useContext(BackendContext);
  if (!ctx) throw new Error("useBackend must be used within BackendProvider");
  return ctx;
}
