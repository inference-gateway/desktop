import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { api, type RouterSettings } from "@/lib/tauri";
import { useBackend } from "@/stores/backend";

const ROW_LABEL = "w-28 shrink-0 text-[0.8rem] font-medium";
const HINT = "text-[0.75rem] text-muted-foreground";
const SELECT =
  "h-8 min-w-0 flex-1 rounded-lg border border-input bg-transparent px-2.5 text-[0.85rem] text-foreground outline-none focus-visible:border-ring";

/** Whether the gateway must restart to serve the Decisions API or stop
 * serving it: the router switched between routing on a model and mentions only. */
function decisionsFlip(before: RouterSettings, next: RouterSettings): boolean {
  return (before.model === "") !== (next.model === "");
}

function blurOnEnter(e: KeyboardEvent<HTMLInputElement>) {
  if (e.key === "Enter") e.currentTarget.blur();
}

/** The rooms router of rooms.yaml, saved as soon as a setting changes: the
 * model that decides whom a post that mentions nobody is for, the bar a member
 * must reach, and what happens when nobody does. */
export function RoomsTab() {
  const { models } = useBackend();
  const [router, setRouter] = useState<RouterSettings | null>(null);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");
  const queue = useRef<Promise<void>>(Promise.resolve());

  useEffect(() => {
    api
      .readRoomsSettings()
      .then((settings) => setRouter(settings.router))
      .catch((e) => setError(String(e)));
  }, []);

  const write = async (next: RouterSettings) => {
    setError("");
    setStatus("Saving...");
    try {
      const { router: before } = await api.readRoomsSettings();
      await api.saveRoomRouter(next);
      if (decisionsFlip(before, next)) {
        setStatus("Restarting the gateway...");
        await api.startGateway(false, true);
      }
      setStatus("Saved");
    } catch (e) {
      setStatus("");
      setError(`Couldn't save: ${e}`);
      setRouter((await api.readRoomsSettings().catch(() => null))?.router ?? null);
    }
  };

  const save = (change: Partial<RouterSettings>) => {
    if (!router) return;
    const next = { ...router, ...change };
    setRouter(next);
    queue.current = queue.current.then(() => write(next));
  };

  return (
    <>
      <h2 className="text-[1.05rem] font-semibold">Rooms</h2>
      <p className="mb-5 text-[0.8rem] text-muted-foreground">
        How a room post that mentions nobody finds its member, saved in ~/.infer/rooms.yaml. Create and edit the rooms
        themselves from the Rooms filter in the sidebar.
      </p>
      {router && (
        <section aria-label="Rooms router" className="flex flex-col gap-3 rounded-lg border border-border bg-card p-3">
          <span role="status" className={`self-end text-[0.75rem] ${error ? "text-err" : "text-muted-foreground"}`}>
            {error || status}
          </span>
          <div className="flex items-center gap-3">
            <Label htmlFor="rooms-router-model" className={ROW_LABEL}>
              Router model
            </Label>
            <select
              id="rooms-router-model"
              value={router.model}
              onChange={(e) => save({ model: e.target.value })}
              className={SELECT}
            >
              <option value="">None - only a mention routes a post</option>
              {router.model && !models.includes(router.model) && (
                <option value={router.model}>{`${router.model} (not served)`}</option>
              )}
              {models.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
          </div>
          <div className="flex items-center gap-3">
            <Label htmlFor="rooms-min-confidence" className={ROW_LABEL}>
              Min confidence
            </Label>
            <Input
              id="rooms-min-confidence"
              type="number"
              min={0}
              max={1}
              step={0.05}
              defaultValue={router.min_confidence}
              onKeyDown={blurOnEnter}
              onBlur={(e) => {
                const value = Number(e.target.value);
                if (value !== router.min_confidence) save({ min_confidence: value });
              }}
              className="w-24"
            />
            <span className={HINT}>How sure the router must be that a member is meant, from 0 to 1.</span>
          </div>
          <div className="flex items-center gap-3">
            <Label htmlFor="rooms-fallback" className={ROW_LABEL}>
              Nobody passes
            </Label>
            <select
              id="rooms-fallback"
              value={router.fallback}
              onChange={(e) => save({ fallback: e.target.value as RouterSettings["fallback"] })}
              className={SELECT}
            >
              <option value="top">Offer it to the most relevant members</option>
              <option value="none">Ask me to mention a member</option>
            </select>
          </div>
        </section>
      )}
      {!router && error && (
        <p role="status" className="text-[0.8rem] text-err">
          {error}
        </p>
      )}
    </>
  );
}
