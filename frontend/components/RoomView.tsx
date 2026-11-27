import { useEffect, useRef } from "react";
import { ArrowRight, Hash, MessageSquare, User } from "lucide-react";
import { cn } from "@/lib/utils";
import { api, type RoomMember, type RoomPost } from "@/lib/tauri";
import { handleLinkClick, renderMarkdown } from "@/lib/markdown";
import { memberChatId, memberLabel, openApprovals, ROOM_AUTHOR, routedTo, USER_AUTHOR } from "@/lib/rooms";
import { useDesktop } from "@/store";
import { useAgents } from "@/stores/agents";
import { useRooms } from "@/stores/rooms";
import { AgentAvatar } from "./AgentAvatar";

const AVATAR_SIZE = 26;

/** The button that opens a member's own chat in the room, where its work, its
 * questions and its approvals live. */
function OpenMemberChat({ room, agent }: { room: string; agent: string }) {
  const { openConversation } = useDesktop();
  return (
    <button
      aria-label={`Open ${agent}'s chat`}
      title={`Open ${agent}'s own chat in #${room}`}
      onClick={() => void openConversation(memberChatId(room, agent))}
      className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[0.72rem] text-muted-foreground hover:bg-foreground/5 hover:text-foreground"
    >
      <MessageSquare size={11} />
      {`${agent}'s chat`}
    </button>
  );
}

function PostAuthor({ post }: { post: RoomPost }) {
  const { agents } = useAgents();
  if (post.author === USER_AUTHOR) return <User size={AVATAR_SIZE * 0.7} className="m-1 text-muted-foreground" />;
  if (post.author === ROOM_AUTHOR) return <Hash size={AVATAR_SIZE * 0.6} className="m-1.5 text-muted-foreground" />;
  const agent = agents.find((a) => a.name === post.author) ?? { name: post.author, emoji: "", avatar: "" };
  return <AgentAvatar agent={agent} size={AVATAR_SIZE} />;
}

function Post({ room, post, members }: { room: string; post: RoomPost; members: Set<string> }) {
  const routes = routedTo(post);
  const byMember = members.has(post.author);
  return (
    <div className="flex gap-2.5 px-4 py-2 hover:bg-foreground/[0.02]" aria-label={`Post by ${post.author}`}>
      <div className="shrink-0">
        <PostAuthor post={post} />
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex flex-wrap items-center gap-1.5 text-[0.78rem]">
          <span className={cn("font-semibold", post.author === ROOM_AUTHOR && "font-normal text-muted-foreground")}>
            {post.author}
          </span>
          {routes.length > 0 && (
            <span className="inline-flex items-center gap-1 text-muted-foreground" title="The members it reached">
              <ArrowRight size={11} />
              {`to ${routes.join(", ")}`}
            </span>
          )}
          {post.time && (
            <span className="text-[0.68rem] text-muted-foreground/60">{new Date(post.time).toLocaleTimeString()}</span>
          )}
          {byMember && <OpenMemberChat room={room} agent={post.author} />}
        </div>
        <div
          className={cn("md text-[0.85rem]", post.author === ROOM_AUTHOR && "text-muted-foreground italic")}
          onClick={(e) => handleLinkClick(e, (url) => api.openUrl(url).catch(() => {}))}
          dangerouslySetInnerHTML={{ __html: renderMarkdown(post.text) }}
        />
        {post.author === USER_AUTHOR && !post.kind && routes.length === 0 && (
          <div className="text-[0.75rem] text-muted-foreground">Reached nobody - mention a member</div>
        )}
        {post.kind === "approval" && post.approval && (
          <div className="text-[0.75rem] text-warn">{`Waiting for approval to run ${post.approval.tool}`}</div>
        )}
      </div>
    </div>
  );
}

/** The members waiting on an approval or a question, each with a way to the
 * chat where it waits. */
function WaitingMembers({ room, members, posts }: { room: string; members: RoomMember[]; posts: RoomPost[] }) {
  const asking = new Set(openApprovals(posts).map((p) => p.author));
  const waiting = members.filter((m) => m.activity === "waiting" || asking.has(m.agent));
  if (waiting.length === 0) return null;
  return (
    <div className="flex flex-wrap items-center gap-2 border-b border-border bg-warn/10 px-4 py-1.5 text-[0.78rem]">
      {waiting.map((m) => (
        <span key={m.agent} className="inline-flex items-center gap-1">
          {`${m.agent} is waiting for you`}
          <OpenMemberChat room={room} agent={m.agent} />
        </span>
      ))}
    </div>
  );
}

/** The open room: who is in it and what they are doing, every post with its
 * author and whom it reached, and the members waiting on the user. */
export function RoomView() {
  const { openRoom } = useRooms();
  const endRef = useRef<HTMLDivElement>(null);
  const posts = openRoom?.posts ?? [];
  useEffect(() => endRef.current?.scrollIntoView({ block: "end" }), [posts.length]);
  if (!openRoom) return null;
  const { room } = openRoom;
  const members = new Set(room.members.map((m) => m.agent));
  return (
    <div id="room" className="flex min-h-0 flex-1 flex-col">
      <header className="flex flex-col gap-0.5 border-b border-border px-4 py-2">
        <h2 className="flex items-center gap-1 text-[0.95rem] font-semibold">
          <Hash size={15} />
          {room.name}
        </h2>
        <p className="truncate text-[0.75rem] text-muted-foreground">
          {room.members.map((m) => memberLabel(m.agent, m.role)).join(", ")}
        </p>
      </header>
      <WaitingMembers room={room.name} members={room.members} posts={posts} />
      <div className="flex-1 overflow-y-auto py-2">
        {posts.length === 0 && (
          <div className="flex flex-col gap-1 px-4 py-6 text-center text-[0.8rem] text-muted-foreground">
            <p>{`No posts in #${room.name} yet.`}</p>
            <p>@ a member, or post and the router picks who answers.</p>
          </div>
        )}
        {posts.map((post) => (
          <Post key={post.id} room={room.name} post={post} members={members} />
        ))}
        <div ref={endRef} />
      </div>
    </div>
  );
}
