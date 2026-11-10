import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";

type Status = { statusText: string; statusError: boolean };

type StatusActions = {
  setStatus: (text: string) => void;
  setError: (text: string) => void;
  /** Drops the error and its message, back to "Ready". */
  clearError: () => void;
  /** Drops the error flag and keeps the text, as a new prompt does. */
  hideError: () => void;
};

const StatusContext = createContext<Status | null>(null);
const StatusActionsContext = createContext<StatusActions | null>(null);

/** The status line and its setters live in two contexts: the setters never
 * change, so code that only reports status does not re-render when the text does. */
export function StatusProvider({ children }: { children: ReactNode }) {
  const [statusText, setStatusText] = useState("");
  const [statusError, setStatusError] = useState(false);

  const setStatus = useCallback((text: string) => {
    setStatusText(text);
    setStatusError(false);
  }, []);
  const setError = useCallback((text: string) => {
    setStatusText(text);
    setStatusError(true);
  }, []);
  const clearError = useCallback(() => {
    setStatusError(false);
    setStatusText((t) => (t.startsWith("Failed") || t.startsWith("Error") ? "Ready" : t));
  }, []);
  const hideError = useCallback(() => setStatusError(false), []);

  const actions = useMemo(
    () => ({ setStatus, setError, clearError, hideError }),
    [setStatus, setError, clearError, hideError],
  );
  const status = useMemo(() => ({ statusText, statusError }), [statusText, statusError]);

  return (
    <StatusActionsContext.Provider value={actions}>
      <StatusContext.Provider value={status}>{children}</StatusContext.Provider>
    </StatusActionsContext.Provider>
  );
}

/** The app's status line: what it says and whether it is an error. */
export function useStatus(): Status {
  const ctx = useContext(StatusContext);
  if (!ctx) throw new Error("useStatus must be used within StatusProvider");
  return ctx;
}

/** Reports status and errors without subscribing to the status text. */
export function useStatusActions(): StatusActions {
  const ctx = useContext(StatusActionsContext);
  if (!ctx) throw new Error("useStatusActions must be used within StatusProvider");
  return ctx;
}
