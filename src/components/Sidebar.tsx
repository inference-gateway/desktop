import { useDesktop } from "@/store";
import { ChatList } from "./ChatList";

export function Sidebar() {
  const { running, newChat } = useDesktop();
  return (
    <aside
      id="sidebar"
      className="flex w-[260px] shrink-0 flex-col gap-2 overflow-hidden border-r border-border bg-secondary p-3"
    >
      <button
        onClick={newChat}
        disabled={running}
        className="rounded-md bg-primary px-3 py-[0.55rem] text-[0.85rem] font-medium text-primary-foreground hover:bg-primary-hover disabled:cursor-not-allowed disabled:opacity-50"
      >
        + New chat
      </button>
      <ChatList />
    </aside>
  );
}
