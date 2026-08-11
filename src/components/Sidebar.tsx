import { useDesktop } from "@/store";
import { ChatList } from "./ChatList";

export function Sidebar() {
  const { newChat } = useDesktop();
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
      <ChatList />
    </aside>
  );
}
