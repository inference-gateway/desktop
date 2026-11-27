import { expect, test } from "bun:test";
import {
  applyRoomFrame,
  memberChatId,
  memberLabel,
  mergePost,
  openApprovals,
  roomOfChat,
  roomSeats,
  routedTo,
  validRoomName,
} from "./rooms";
import type { RoomPost } from "./tauri";

const post = (id: string, extra: Partial<RoomPost> = {}): RoomPost => ({ id, author: "user", text: id, ...extra });

const release = {
  room: {
    name: "release",
    members: [
      { agent: "tester", session_id: "room-release-tester", activity: "idle" as const },
      { agent: "writer", session_id: "room-release-writer", activity: "idle" as const },
    ],
  },
  posts: [post("1")],
};

test("a member chat names its room up to the first dash", () => {
  expect(memberChatId("release", "code-reviewer")).toBe("room-release-code-reviewer");
  expect(roomOfChat("room-release-code-reviewer")).toBe("release");
  expect(roomOfChat("8c1f-chat")).toBeNull();
  expect(roomOfChat("room-")).toBeNull();
});

test("a post the poster already got is not added twice", () => {
  expect(mergePost([post("1")], post("1")).map((p) => p.id)).toEqual(["1"]);
  expect(mergePost([post("1")], post("2")).map((p) => p.id)).toEqual(["1", "2"]);
});

test("a follow frame adds a post, moves a member's activity or clears the room", () => {
  const posted = applyRoomFrame(release, { type: "room_post", room: "release", post: post("2") });
  expect(posted.posts.map((p) => p.id)).toEqual(["1", "2"]);

  const waiting = applyRoomFrame(release, {
    type: "room_activity",
    room: "release",
    member: "tester",
    activity: "waiting",
  });
  expect(waiting.room.members.map((m) => m.activity)).toEqual(["waiting", "idle"]);

  expect(applyRoomFrame(release, { type: "room_cleared", room: "release" }).posts).toEqual([]);
  expect(applyRoomFrame(release, { type: "room_cleared", room: "public" })).toBe(release);
});

test("an approval stays open until a decision post closes its call", () => {
  const asked = post("a", { author: "tester", kind: "approval", approval: { id: "call_7", tool: "Bash" } });
  const other = post("b", { author: "writer", kind: "approval", approval: { id: "call_8", tool: "Write" } });
  const decided = post("c", { author: "room", kind: "decision", approval: { id: "call_7", tool: "Bash" } });
  expect(openApprovals([asked, other]).map((p) => p.id)).toEqual(["a", "b"]);
  expect(openApprovals([asked, other, decided]).map((p) => p.id)).toEqual(["b"]);
});

test("a post names the members it was routed to and a member its role", () => {
  expect(
    routedTo(
      post("1", {
        routes: [
          { member: "tester", probability: 1 },
          { member: "writer", probability: 0.7 },
        ],
      }),
    ),
  ).toEqual(["tester", "writer"]);
  expect(routedTo(post("1"))).toEqual([]);
  expect(memberLabel("jeff", "tester")).toBe("jeff (tester)");
  expect(memberLabel("jeff")).toBe("jeff");
});

test("a room name keeps to letters, digits and underscores after a letter or digit", () => {
  expect(validRoomName("release_2")).toBe(true);
  expect(validRoomName("2026")).toBe(true);
  expect(validRoomName("")).toBe(false);
  expect(validRoomName("_release")).toBe(false);
  expect(validRoomName("re-lease")).toBe(false);
});

test("all agents seats the wildcard and keeps only the built-ins picked beside it", () => {
  const builtIns = new Set(["main", "explore"]);
  expect(roomSeats(true, ["reviewer", "main"], builtIns)).toEqual(["*", "main"]);
  expect(roomSeats(false, ["reviewer", "main"], builtIns)).toEqual(["reviewer", "main"]);
  expect(roomSeats(false, ["*", "reviewer"], builtIns)).toEqual(["reviewer"]);
});
