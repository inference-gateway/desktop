import {
  createContext,
  useContext,
  useMemo,
  useState,
  type Dispatch,
  type ReactNode,
  type SetStateAction,
} from "react";
import type { ProjectType } from "@/lib/sidebar-document";

export type View = "chat" | "settings" | "observability" | "computers";
/** What the sidebar lists: every project, one project type, or the agents. */
export type SidebarFilter = ProjectType | "agents";

type Setter<T> = Dispatch<SetStateAction<T>>;

/** Where the window is: the current view, whether the sidebar and chat panes
 * are open, the sidebar's filter, and the Settings tab and project filter the
 * next Settings visit opens on. */
type Navigation = {
  currentView: View;
  setCurrentView: Setter<View>;
  sidebarOpen: boolean;
  setSidebarOpen: Setter<boolean>;
  chatOpen: boolean;
  setChatOpen: Setter<boolean>;
  projectFilter: SidebarFilter | null;
  setProjectFilter: Setter<SidebarFilter | null>;
  initialSettingsTab: string;
  setInitialSettingsTab: Setter<string>;
  initialProjectFilter: string;
  setInitialProjectFilter: Setter<string>;
};

const NavigationContext = createContext<Navigation | null>(null);

/** Navigation lives apart from the main store, so its readers re-render only
 * when the user moves around, not on every agent event. */
export function NavigationProvider({ children }: { children: ReactNode }) {
  const [currentView, setCurrentView] = useState<View>("chat");
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [chatOpen, setChatOpen] = useState(true);
  const [projectFilter, setProjectFilter] = useState<SidebarFilter | null>(null);
  const [initialSettingsTab, setInitialSettingsTab] = useState("general");
  const [initialProjectFilter, setInitialProjectFilter] = useState("");

  const navigation = useMemo(
    () => ({
      currentView,
      setCurrentView,
      sidebarOpen,
      setSidebarOpen,
      chatOpen,
      setChatOpen,
      projectFilter,
      setProjectFilter,
      initialSettingsTab,
      setInitialSettingsTab,
      initialProjectFilter,
      setInitialProjectFilter,
    }),
    [currentView, sidebarOpen, chatOpen, projectFilter, initialSettingsTab, initialProjectFilter],
  );

  return <NavigationContext.Provider value={navigation}>{children}</NavigationContext.Provider>;
}

export function useNavigation(): Navigation {
  const ctx = useContext(NavigationContext);
  if (!ctx) throw new Error("useNavigation must be used within NavigationProvider");
  return ctx;
}
