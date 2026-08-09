import { useDesktop } from "@/store";
import { ChatList } from "./ChatList";

export function Sidebar() {
  const { newChat, setCurrentView } = useDesktop();
  return (
    <aside
      id="sidebar"
      className="flex w-[260px] shrink-0 flex-col gap-2 overflow-hidden border-r border-border bg-secondary p-3"
    >
      <button
        onClick={newChat}
        className="rounded-md bg-primary px-3 py-[0.55rem] text-[0.85rem] font-medium text-primary-foreground hover:bg-primary-hover"
      >
        + New chat
      </button>
      <button
        onClick={() => setCurrentView("settings")}
        className="rounded-md px-3 py-[0.55rem] text-[0.85rem] font-medium text-left hover:bg-primary/10 disabled:cursor-not-allowed disabled:opacity-50"
      >
        Settings
      </button>
      <ChatList />
    </aside>
  );
}
