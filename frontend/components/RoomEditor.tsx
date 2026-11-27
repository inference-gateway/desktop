import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { api, type RoomAgent } from "@/lib/tauri";
import { ALL_AGENTS, roomSeats, validRoomName } from "@/lib/rooms";
import { useRooms } from "@/stores/rooms";
import { useSidebar } from "@/stores/sidebar";
import { useStatusActions } from "@/stores/status";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

const SELECT =
  "h-8 rounded-lg border border-input bg-transparent px-2.5 text-[0.85rem] text-foreground outline-none focus-visible:border-ring";

/** New room or Edit room: its name, the project its members run in and who is
 * in it, saved to rooms.yaml. A room the file does not hold yet, such as the
 * default `public`, is written there on its first save. */
export function RoomEditor() {
  const { roomEditor, closeRoomEditor, saveRoom } = useRooms();
  const { projectNames } = useSidebar();
  const { setError } = useStatusActions();
  const existing = roomEditor?.room ?? null;
  const [name, setName] = useState(existing?.name ?? "");
  const [project, setProject] = useState(existing?.project_name ?? existing?.project ?? "");
  const [allAgents, setAllAgents] = useState(existing ? existing.agents.includes(ALL_AGENTS) : true);
  const [picked, setPicked] = useState<string[]>(existing?.agents ?? []);
  const [listedAgents, setListedAgents] = useState<RoomAgent[] | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    api
      .listRoomAgents()
      .then(setListedAgents)
      .catch((e) => setError(`Agents: ${e}`));
  }, [setError]);

  const agents = listedAgents ?? [];
  const builtIns = new Set(agents.filter((a) => a.built_in).map((a) => a.name));
  const listed = new Set(agents.map((a) => a.name));
  const missing = listedAgents ? picked.filter((n) => n !== ALL_AGENTS && !listed.has(n)) : [];
  const seats = roomSeats(allAgents, picked, builtIns);
  const complete = validRoomName(name) && seats.length > 0;
  const unmatchedDir = existing && !existing.project_name && existing.project ? existing.project : null;

  const toggle = (agent: string, on: boolean) =>
    setPicked((prev) => (on ? [...prev, agent] : prev.filter((n) => n !== agent)));

  const save = async () => {
    setSaving(true);
    try {
      await saveRoom({ name, project, agents: seats }, existing?.name ?? null);
    } catch (e) {
      setError(`Save room: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div id="room-editor" className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="mx-auto flex w-full max-w-[44rem] flex-col gap-5 px-6 py-8">
        <div className="flex items-start justify-between gap-4">
          <h2 className="text-[1.1rem] font-semibold">{existing ? `Edit #${existing.name}` : "New room"}</h2>
          <button
            aria-label="Cancel room editor"
            title="Cancel"
            onClick={closeRoomEditor}
            className="rounded-md p-1 text-muted-foreground hover:bg-secondary hover:text-foreground"
          >
            <X size={16} />
          </button>
        </div>

        <div className="flex flex-col gap-1">
          <Label htmlFor="room-name" className="text-[0.8rem] text-muted-foreground">
            Name
          </Label>
          <Input
            id="room-name"
            value={name}
            placeholder="release"
            autoFocus={!existing}
            onKeyDown={(e) => e.key === "Enter" && document.getElementById("room-project")?.focus()}
            onChange={(e) => setName(e.target.value.trim())}
          />
          <span className="text-[0.72rem] text-muted-foreground/70">
            {name && !validRoomName(name)
              ? "Start with a letter or digit and keep to letters, digits and underscores."
              : `@ the members in #${name || "<name>"}. Saved in ~/.infer/rooms.yaml.`}
          </span>
        </div>

        <div className="flex flex-col gap-1">
          <Label htmlFor="room-project" className="text-[0.8rem] text-muted-foreground">
            Project
          </Label>
          <select id="room-project" value={project} onChange={(e) => setProject(e.target.value)} className={SELECT}>
            <option value="">No project - the daemon's default folder</option>
            {unmatchedDir && <option value={unmatchedDir}>{unmatchedDir}</option>}
            {projectNames.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
          <span className="text-[0.72rem] text-muted-foreground/70">The folder the members work in.</span>
        </div>

        <fieldset className="flex flex-col gap-1">
          <legend className="mb-1 text-[0.8rem] text-muted-foreground">Members</legend>
          <label className="flex items-center gap-2 text-[0.82rem]">
            <input
              type="checkbox"
              id="room-all-agents"
              checked={allAgents}
              onChange={(e) => setAllAgents(e.target.checked)}
              className="h-4 w-4 accent-primary"
            />
            All agents
          </label>
          <span className="mb-1 text-[0.72rem] text-muted-foreground/70">
            Every agent of your own, new ones too. Built-in agents join only when picked.
          </span>
          <div className="flex flex-wrap gap-x-4 gap-y-1.5">
            {[...agents, ...missing.map((n) => ({ name: n, built_in: false }))].map((agent) => {
              const seatedByAll = allAgents && !agent.built_in;
              return (
                <label key={agent.name} className="flex items-center gap-2 text-[0.82rem]">
                  <input
                    type="checkbox"
                    id={`room-agent-${agent.name}`}
                    checked={seatedByAll || picked.includes(agent.name)}
                    disabled={seatedByAll}
                    onChange={(e) => toggle(agent.name, e.target.checked)}
                    className="h-4 w-4 accent-primary"
                  />
                  {agent.name}
                  {agent.built_in && <span className="text-muted-foreground/70">(built-in)</span>}
                  {!listed.has(agent.name) && <span className="text-warn">(missing)</span>}
                </label>
              );
            })}
          </div>
        </fieldset>

        <div className="flex items-center justify-end gap-2">
          <Button variant="ghost" onClick={closeRoomEditor}>
            Cancel
          </Button>
          <Button disabled={!complete || saving} onClick={save}>
            Save room
          </Button>
        </div>
      </div>
    </div>
  );
}
