import { useMemo, useState } from "react";
import { Sparkles } from "lucide-react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ChatList } from "./ChatList";

function CheckboxGlyph({ checked }: { checked: boolean }) {
  return (
    <span
      aria-hidden
      className={cn(
        "inline-block size-3.5 shrink-0 rounded-[4px] border",
        checked ? "border-primary bg-primary" : "border-muted-foreground/50"
      )}
    />
  );
}

/** Confirm what a bulk /init will touch, with per-project and per-group opt-outs. */
function InitAllDialog({
  open,
  onOpenChange,
  onConfirm,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: (projectNames: string[]) => void;
}) {
  const { projectNames, projectGroups } = useDesktop();
  // null = freshly opened dialog, every project selected by default.
  const [picked, setPicked] = useState<Set<string> | null>(null);
  const selected = picked ?? new Set(projectNames);

  const flip = (names: string[], on: boolean) =>
    setPicked((prev) => {
      const next = new Set(prev ?? projectNames);
      if (on) for (const n of names) next.add(n);
      else for (const n of names) next.delete(n);
      return next;
    });
  const toggleGroup = (names: string[]) => flip(names, !names.every((n) => selected.has(n)));

  // Projects grouped by their group label ("" = ungrouped), like the sidebar.
  const clusters = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const name of projectNames) {
      const label = projectGroups[name] ?? "";
      (map.get(label) ?? map.set(label, []).get(label)!).push(name);
    }
    return Array.from(map.entries()).sort(([a], [b]) => a.localeCompare(b));
  }, [projectNames, projectGroups]);

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (o) setPicked(null);
        onOpenChange(o);
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Init all projects</DialogTitle>
          <DialogDescription>
            One /init conversation per selected project. Git-backed projects also open a pull
            request with the AGENTS.md changes for review - nothing is merged automatically.
          </DialogDescription>
        </DialogHeader>
        <div className="max-h-64 overflow-y-auto">
          {projectNames.length === 0 ? (
            <p className="py-2 text-sm text-muted-foreground">No projects yet</p>
          ) : (
            clusters.map(([label, names]) => {
              const allIn = names.every((n) => selected.has(n));
              return (
                <div key={label || "(ungrouped)"} className="py-1">
                  {label && (
                    <button
                      aria-label={`Toggle init for group ${label}`}
                      aria-pressed={allIn}
                      onClick={() => toggleGroup(names)}
                      className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-[0.8rem] font-semibold text-foreground/90 hover:bg-muted"
                    >
                      <CheckboxGlyph checked={allIn} />
                      {label}
                    </button>
                  )}
                  <div className={label ? "pl-4" : undefined}>
                    {names.map((name) => (
                      <button
                        key={name}
                        aria-label={`Toggle init for project ${name}`}
                        aria-pressed={selected.has(name)}
                        onClick={() => flip([name], !selected.has(name))}
                        className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-[0.83rem] text-muted-foreground hover:bg-muted"
                      >
                        <CheckboxGlyph checked={selected.has(name)} />
                        <span className="min-w-0 truncate">{name}</span>
                      </button>
                    ))}
                  </div>
                </div>
              );
            })
          )}
        </div>
        <p className="text-[0.78rem] text-muted-foreground/70">
          {selected.size} of {projectNames.length} selected
        </p>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button
            disabled={projectNames.length === 0 || selected.size === 0}
            onClick={() => {
              onOpenChange(false);
              onConfirm(Array.from(selected));
            }}
          >
            Start init
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Prompt to create a new project shown below the chat list */
export function Sidebar() {
  const { newChat, initAllProjects, initAllRunning } = useDesktop();
  const [initOpen, setInitOpen] = useState(false);
  return (
    <aside
      id="sidebar"
      className="flex w-[320px] shrink-0 flex-col gap-2 overflow-hidden border-r border-border bg-secondary p-3"
    >
      <div className="flex gap-2">
        <Button
          onClick={newChat}
          title="New chat (⌘N / Ctrl+N)"
          className="flex-1 text-[0.85rem]"
        >
          + New chat
        </Button>
        <Button
          size="icon"
          aria-label="Init all projects"
          title="Init all projects"
          disabled={initAllRunning}
          onClick={() => setInitOpen(true)}
        >
          <Sparkles />
        </Button>
      </div>
      <ChatList />
      <InitAllDialog open={initOpen} onOpenChange={setInitOpen} onConfirm={initAllProjects} />
    </aside>
  );
}