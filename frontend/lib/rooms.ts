import type { Room, RoomFrame, RoomPost } from "./tauri";

/** Who wrote a post that is not a member's: the user, or the room's own notes. */
export const USER_AUTHOR = "user";
export const ROOM_AUTHOR = "room";

const MEMBER_CHAT_PREFIX = "room-";

/** The seat that keeps every agent of the user's own in a room, new ones too. */
export const ALL_AGENTS = "*";

/** The room every agent is in until rooms.yaml says otherwise. */
export const DEFAULT_ROOM = "public";

const ROOM_NAME = /^[a-zA-Z0-9][a-zA-Z0-9_]*$/;

/** A followed room as the view shows it: the room with its members' activity,
 * and its posts in log order. */
export type RoomState = { room: Room; posts: RoomPost[] };

/** The session a member keeps in a room, `room-<room>-<agent>`. */
export function memberChatId(room: string, agent: string): string {
  return `${MEMBER_CHAT_PREFIX}${room}-${agent}`;
}

/** The room a member chat belongs to, or null for any other chat. A room name
 * has no dash, so the room part ends at the first one. */
export function roomOfChat(id: string): string | null {
  if (!id.startsWith(MEMBER_CHAT_PREFIX)) return null;
  const rest = id.slice(MEMBER_CHAT_PREFIX.length);
  const dash = rest.indexOf("-");
  return dash > 0 ? rest.slice(0, dash) : null;
}

/** The posts with `post` added, once: the poster gets its post both as the
 * answer to its post and from the follow. */
export function mergePost(posts: RoomPost[], post: RoomPost): RoomPost[] {
  return posts.some((p) => p.id === post.id) ? posts : [...posts, post];
}

/** The room after one frame of its follow. A frame of another room, or of a
 * kind the view does not show, leaves it as it was. */
export function applyRoomFrame(state: RoomState, frame: RoomFrame): RoomState {
  if (frame.room !== state.room.name) return state;
  switch (frame.type) {
    case "room_post":
      return { ...state, posts: mergePost(state.posts, frame.post) };
    case "room_activity":
      return {
        ...state,
        room: {
          ...state.room,
          members: state.room.members.map((m) => (m.agent === frame.member ? { ...m, activity: frame.activity } : m)),
        },
      };
    case "room_cleared":
      return { ...state, posts: [] };
    default:
      return state;
  }
}

/** The approval posts no `decision` post has closed yet, oldest first. */
export function openApprovals(posts: RoomPost[]): RoomPost[] {
  const closed = new Set(posts.filter((p) => p.kind === "decision").map((p) => p.approval?.id));
  return posts.filter((p) => p.kind === "approval" && p.approval && !closed.has(p.approval.id));
}

/** The members a post was routed to, in route order. */
export function routedTo(post: RoomPost): string[] {
  return (post.routes ?? []).map((r) => r.member);
}

/** `jeff (tester)` for a member whose agent names a role, else the bare name. */
export function memberLabel(agent: string, role?: string): string {
  return role ? `${agent} (${role})` : agent;
}

/** Whether `name` may name a room: a letter or digit, then letters, digits and
 * underscores, so a member's chat id splits at its first dash. */
export function validRoomName(name: string): boolean {
  return ROOM_NAME.test(name);
}

/** The seats a room's editor saves: `*` when every agent of the user's own is
 * in, plus the built-ins picked beside it, else just the picked agents. */
export function roomSeats(allAgents: boolean, picked: string[], builtIns: Set<string>): string[] {
  const named = picked.filter((name) => name !== ALL_AGENTS);
  return allAgents ? [ALL_AGENTS, ...named.filter((name) => builtIns.has(name))] : named;
}
