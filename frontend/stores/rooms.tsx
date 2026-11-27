import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, Channel, type Room, type RoomEdit, type RoomEntry, type RoomFrame } from "@/lib/tauri";
import { applyRoomFrame, mergePost, type RoomState } from "@/lib/rooms";
import { useBackend } from "@/stores/backend";
import { useStatusActions } from "@/stores/status";

function useRoomsStore() {
  const { ready } = useBackend();
  const { setStatus, setError } = useStatusActions();
  const [rooms, setRooms] = useState<Room[]>([]);
  const [openRoom, setOpenRoom] = useState<RoomState | null>(null);
  const [roomEditor, setRoomEditor] = useState<{ room: RoomEntry | null } | null>(null);
  const followed = useRef<string | null>(null);

  const loadRooms = useCallback(async () => {
    try {
      setRooms(await api.listRooms());
    } catch (e) {
      setError(`Rooms: ${e}`);
    }
  }, [setError]);

  useEffect(() => {
    if (ready) void loadRooms();
  }, [ready, loadRooms]);

  const leave = useCallback(() => {
    if (followed.current) void api.leaveRoom(followed.current).catch(() => {});
    followed.current = null;
  }, []);

  const closeRoom = useCallback(() => {
    leave();
    setOpenRoom(null);
    setRoomEditor(null);
  }, [leave]);

  const enterRoom = useCallback(
    async (name: string) => {
      leave();
      setRoomEditor(null);
      followed.current = name;
      setOpenRoom({ room: rooms.find((r) => r.name === name) ?? { name, members: [] }, posts: [] });
      const early: RoomFrame[] = [];
      let following = false;
      const channel = new Channel<RoomFrame>();
      channel.onmessage = (frame) => {
        if (following) setOpenRoom((state) => state && applyRoomFrame(state, frame));
        else early.push(frame);
      };
      try {
        const snapshot = await api.followRoom(name, channel);
        if (followed.current !== name) return;
        setOpenRoom((state) =>
          early.reduce(applyRoomFrame, {
            room: snapshot.room,
            posts: (state?.posts ?? []).reduce(mergePost, snapshot.posts ?? []),
          }),
        );
        following = true;
      } catch (e) {
        if (followed.current === name) closeRoom();
        setError(`Room ${name}: ${e}`);
      }
    },
    [rooms, leave, closeRoom, setError],
  );

  const postToRoom = useCallback(
    async (text: string) => {
      const name = followed.current;
      if (!name || !text.trim()) return;
      try {
        const posted = await api.postRoom(name, text);
        setOpenRoom((state) =>
          state?.room.name === name ? { ...state, posts: mergePost(state.posts, posted.post) } : state,
        );
        if (posted.notices?.length) setStatus(posted.notices.join(" "));
      } catch (e) {
        setError(`Post to ${name}: ${e}`);
      }
    },
    [setStatus, setError],
  );

  const openRoomEditor = useCallback(
    async (name: string | null) => {
      leave();
      setOpenRoom(null);
      if (name === null) {
        setRoomEditor({ room: null });
        return;
      }
      try {
        const { rooms: configured } = await api.readRoomsSettings();
        setRoomEditor({ room: configured.find((r) => r.name === name) ?? null });
      } catch (e) {
        setError(`rooms.yaml: ${e}`);
      }
    },
    [leave, setError],
  );

  const closeRoomEditor = useCallback(() => setRoomEditor(null), []);

  const saveRoom = useCallback(
    async (room: RoomEdit, previous: string | null) => {
      await api.saveRoom(room, previous);
      setRoomEditor(null);
      await loadRooms();
    },
    [loadRooms],
  );

  const deleteRoom = useCallback(
    async (name: string) => {
      try {
        await api.deleteRoom(name);
        if (followed.current === name) closeRoom();
        await loadRooms();
      } catch (e) {
        setError(`Delete room ${name}: ${e}`);
      }
    },
    [closeRoom, loadRooms, setError],
  );

  return useMemo(
    () => ({
      rooms,
      loadRooms,
      openRoom,
      enterRoom,
      closeRoom,
      postToRoom,
      roomEditor,
      openRoomEditor,
      closeRoomEditor,
      saveRoom,
      deleteRoom,
    }),
    [
      rooms,
      loadRooms,
      openRoom,
      enterRoom,
      closeRoom,
      postToRoom,
      roomEditor,
      openRoomEditor,
      closeRoomEditor,
      saveRoom,
      deleteRoom,
    ],
  );
}

type RoomsStore = ReturnType<typeof useRoomsStore>;

const RoomsContext = createContext<RoomsStore | null>(null);

export function RoomsProvider({ children }: { children: ReactNode }) {
  return <RoomsContext.Provider value={useRoomsStore()}>{children}</RoomsContext.Provider>;
}

/** The rooms of rooms.yaml, listed once the backend is ready, the room open
 * in the main pane, followed on its own connection and left when another room
 * or a chat opens, and the room editor, which writes rooms.yaml. */
export function useRooms(): RoomsStore {
  const ctx = useContext(RoomsContext);
  if (!ctx) throw new Error("useRooms must be used within RoomsProvider");
  return ctx;
}
