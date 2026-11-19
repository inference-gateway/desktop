import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type MutableRefObject,
  type ReactNode,
  type SetStateAction,
} from "react";

/** The chats picked in the sidebar for a bulk action, and the row a
 * shift-click extends the range from. */
type Selection = {
  selected: Set<string>;
  setSelected: Dispatch<SetStateAction<Set<string>>>;
  lastClickedIndex: MutableRefObject<number>;
  clearSelection: () => void;
};

const SelectionContext = createContext<Selection | null>(null);

export function SelectionProvider({ children }: { children: ReactNode }) {
  const [selected, setSelected] = useState<Set<string>>(() => new Set());
  const lastClickedIndex = useRef(-1);

  const clearSelection = useCallback(() => {
    setSelected(new Set());
    lastClickedIndex.current = -1;
  }, []);

  const selection = useMemo(
    () => ({ selected, setSelected, lastClickedIndex, clearSelection }),
    [selected, clearSelection],
  );

  return <SelectionContext.Provider value={selection}>{children}</SelectionContext.Provider>;
}

export function useSelection(): Selection {
  const ctx = useContext(SelectionContext);
  if (!ctx) throw new Error("useSelection must be used within SelectionProvider");
  return ctx;
}
