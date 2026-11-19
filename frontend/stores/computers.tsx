import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type MutableRefObject,
  type ReactNode,
  type SetStateAction,
} from "react";

/** The chat whose computer screen the user drives, and the chats moving
 * between computers with the stage each move is at. */
type Computers = {
  drivenId: string | null;
  setDrivenId: Dispatch<SetStateAction<string | null>>;
  moving: Record<string, string>;
  setMoving: Dispatch<SetStateAction<Record<string, string>>>;
  movingRef: MutableRefObject<Record<string, string>>;
};

const ComputersContext = createContext<Computers | null>(null);

export function ComputersProvider({ children }: { children: ReactNode }) {
  const [drivenId, setDrivenId] = useState<string | null>(null);
  const [moving, setMoving] = useState<Record<string, string>>(() => ({}));
  const movingRef = useRef(moving);
  useEffect(() => {
    movingRef.current = moving;
  }, [moving]);

  const computers = useMemo(() => ({ drivenId, setDrivenId, moving, setMoving, movingRef }), [drivenId, moving]);

  return <ComputersContext.Provider value={computers}>{children}</ComputersContext.Provider>;
}

export function useComputers(): Computers {
  const ctx = useContext(ComputersContext);
  if (!ctx) throw new Error("useComputers must be used within ComputersProvider");
  return ctx;
}
