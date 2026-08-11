import { useMemo, useState, type DragEvent } from "react";
import { FolderPlus, Trash2, ChevronRight, ChevronDown, Pencil, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";

function ChatItem({ index }: { index: number }) {
  const { conversations, selected, sessionId, onChatClick, deleteConversation, isRunning, isAwaitingApproval } =
    useDesktop();
  const conv = conversations[index];
  const [confirm, setConfirm] = useState(false);
  const isSelected = selected.has(conv.id);
  const isActive = conv.id === sessionId;
  const running = isRunning(conv.id);
  const awaiting = isAwaitingApproval(conv.id);

  return (
    <div
      role="listitem"
      title={conv.title || conv.id}
      aria-selected={isSelected || undefined}
      onClick={(e) => onChatClick(index, e)}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.setData("text/plain", conv.id);
        e.dataTransfer.effectAllowed = "move";
      }}
      className={cn(
        "group flex cursor-pointer items-center gap-1 rounded-md px-[0.6rem] py-2 text-[0.83rem] text-muted-foreground hover:bg-card hover:text-foreground",
        isActive && "bg-card font-medium text-foreground shadow-[inset_3px_0_0_var(--primary)]",
        isSelected && "bg-primary/10 text-foreground shadow-[inset_3px_0_0_var(--primary)] ring-1 ring-primary/25"
      )}
    >
      {running && (
        <span
          aria-label={awaiting ? "Awaiting approval" : "Running"}
          title={awaiting ? "Awaiting approval" : "Running"}
          className={cn(
            "h-2 w-2 shrink-0 rounded-full",
            awaiting ? "bg-amber-500" : "animate-pulse bg-emerald-500"
          )}
        />
      )}
      <span className="min-w-0 flex-1 truncate">{conv.title || "(untitled)"}</span>
      <button
        aria-label="Delete conversation"
        title={confirm ? "Click again to delete" : "Delete conversation"}
        onClick={(e) => {
          e.stopPropagation();
          if (!confirm) {
            setConfirm(true);
            return;
          }
          deleteConversation(conv.id);
        }}
        onMouseLeave={() => setConfirm(false)}
        className={cn(
          "inline-flex shrink-0 items-center justify-center rounded-md p-[0.15rem] opacity-0 group-hover:opacity-100 focus-visible:opacity-100",
          confirm ? "bg-destructive text-white opacity-100" : "text-muted-foreground hover:bg-card hover:text-destructive"
        )}
      >
        <Trash2 size={14} />
      </button>
    </div>
  );
}

function BulkDeleteBar({ count, onDelete }: { count: number; onDelete: () => void }) {
  const [armed, setArmed] = useState(false);
  return (
    <div className="shrink-0 border-t border-border py-2">
      <button
        onClick={() => {
          if (!armed) {
            setArmed(true);
            return;
          }
          onDelete();
        }}
        onMouseLeave={() => setArmed(false)}
        className="w-full rounded-md bg-destructive px-3 py-2 text-[0.83rem] font-medium text-white hover:bg-danger-hover"
      >
        {armed ? `Click again to delete ${count}` : `Delete ${count} selected`}
      </button>
    </div>
  );
}

function ProjectGroup({
  name,
  count,
  collapsed,
  onToggle,
  onRename,
  onDelete,
  onDrop,
  onDragOverProject,
  children,
}: {
  name: string;
  count: number;
  collapsed: boolean;
  onToggle: () => void;
  onRename: (newName: string) => void;
  onDelete: () => void;
  onDrop: (e: DragEvent) => void;
  onDragOverProject: (over: boolean) => void;
  children: React.ReactNode;
}) {
  const [editing, setEditing] = useState(false);
  const [editValue, setEditValue] = useState(name);

  const handleSubmit = () => {
    const trimmed = editValue.trim();
    if (trimmed && trimmed !== name) onRename(trimmed);
    setEditing(false);
  };

  return (
    <div
      onDragOver={(e) => {
        e.preventDefault();
        e.dataTransfer.dropEffect = "move";
        onDragOverProject(true);
      }}
      onDragLeave={() => onDragOverProject(false)}
      onDrop={(e) => {
        onDrop(e);
        onDragOverProject(false);
      }}
      className="group/project"
    >
      <div
        className={cn(
          "flex cursor-pointer items-center gap-1 rounded-md px-[0.4rem] py-1.5 text-[0.8rem] font-semibold uppercase tracking-wide text-muted-foreground hover:bg-card",
        )}
      >
        <button
          onClick={onToggle}
          aria-label={collapsed ? `Expand ${name}` : `Collapse ${name}`}
          className="inline-flex items-center gap-1 truncate"
        >
          {collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
          {editing ? (
            <input
              value={editValue}
              onChange={(e) => setEditValue(e.target.value)}
              onBlur={handleSubmit}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleSubmit();
                if (e.key === "Escape") setEditing(false);
              }}
              autoFocus
              className="w-full bg-transparent text-foreground outline-none"
              onClick={(e) => e.stopPropagation()}
            />
          ) : (
            <span onDoubleClick={(e) => { e.stopPropagation(); setEditing(true); setEditValue(name); }}>
              {name}
            </span>
          )}
          <span className="ml-1 text-[0.7rem] text-muted-foreground/60">({count})</span>
        </button>
        <div className="ml-auto flex items-center gap-0.5 opacity-0 group-hover/project:opacity-100 focus-within:opacity-100">
          <button
            aria-label={`Rename project ${name}`}
            title="Rename"
            onClick={(e) => { e.stopPropagation(); setEditing(true); setEditValue(name); }}
            className="inline-flex items-center justify-center rounded p-[0.15rem] text-muted-foreground hover:text-foreground"
          >
            <Pencil size={11} />
          </button>
          <button
            aria-label={`Delete project ${name}`}
            title="Delete project"
            onClick={(e) => {
              e.stopPropagation();
              onDelete();
            }}
            className="inline-flex items-center justify-center rounded p-[0.15rem] text-muted-foreground hover:text-destructive"
          >
            <X size={11} />
          </button>
        </div>
      </div>
      {!collapsed && <div className="flex flex-col gap-[2px] pl-2">{children}</div>}
    </div>
  );
}

export function ChatList() {
  const {
    conversations,
    selected,
    projects,
    projectNames,
    collapsedProjects,
    bulkDelete,
    assignProject,
    unassignProject,
    deleteProject,
    renameProject,
    toggleCollapseProject,
    createProject,
  } = useDesktop();

  const [newProjectInput, setNewProjectInput] = useState(false);
  const [newProjectName, setNewProjectName] = useState("");
  const [dragOverUngrouped, setDragOverUngrouped] = useState(false);
  const [dragOverProject, setDragOverProject] = useState<string | null>(null);

  const groups = useMemo(() => {
    const map: Record<string, number[]> = {};
    for (const name of projectNames) {
      map[name] = [];
    }
    const ungrouped: number[] = [];
    conversations.forEach((conv, i) => {
      const p = projects[conv.id];
      if (p && map[p]) {
        map[p].push(i);
      } else {
        ungrouped.push(i);
      }
    });
    // Only show projects that have sessions (or are new with a heading)
    return {
      projects: Object.entries(map).filter(([, indices]) => indices.length > 0),
      ungrouped,
    };
  }, [conversations, projects, projectNames]);

  const handleDropOnProject = (e: DragEvent, projectName: string) => {
    e.preventDefault();
    const sessionId = e.dataTransfer.getData("text/plain");
    if (sessionId) assignProject(sessionId, projectName);
  };

  const handleDropOnUngrouped = (e: DragEvent) => {
    e.preventDefault();
    const sessionId = e.dataTransfer.getData("text/plain");
    if (sessionId) unassignProject(sessionId);
  };

  const handleNewProject = () => {
    const trimmed = newProjectName.trim();
    if (trimmed) {
      createProject(trimmed);
      setNewProjectInput(false);
      setNewProjectName("");
    }
  };

  return (
    <>
      <div id="chat-list" role="list" className="flex flex-1 flex-col gap-[2px] overflow-y-auto">
        {groups.projects.map(([name, indices]) => {
          const collapsed = collapsedProjects.has(name);
          return (
            <ProjectGroup
              key={name}
              name={name}
              count={indices.length}
              collapsed={collapsed}
              onToggle={() => toggleCollapseProject(name)}
              onRename={(n) => renameProject(name, n)}
              onDelete={() => deleteProject(name)}
              onDrop={(e) => handleDropOnProject(e, name)}
              onDragOverProject={(over) => setDragOverProject(over ? name : null)}
            >
              {indices.map((i) => (
                <ChatItem key={conversations[i]?.id ?? i} index={i} />
              ))}
            </ProjectGroup>
          );
        })}

        {groups.ungrouped.length > 0 && (
          <div
            onDragOver={(e) => {
              e.preventDefault();
              e.dataTransfer.dropEffect = "move";
              setDragOverUngrouped(true);
            }}
            onDragLeave={() => setDragOverUngrouped(false)}
            onDrop={handleDropOnUngrouped}
            className={cn(
              "flex flex-col gap-[2px] rounded-md",
              dragOverUngrouped && "ring-1 ring-primary/30"
            )}
          >
            {groups.ungrouped.map((i) => (
              <ChatItem key={conversations[i]?.id ?? i} index={i} />
            ))}
          </div>
        )}

        {conversations.length === 0 && (
          <p className="px-[0.6rem] py-4 text-center text-[0.8rem] text-muted-foreground">
            No conversations yet
          </p>
        )}
      </div>

      {newProjectInput ? (
        <div className="shrink-0 border-t border-border px-1 py-2">
          <input
            value={newProjectName}
            onChange={(e) => setNewProjectName(e.target.value)}
            onBlur={handleNewProject}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleNewProject();
              if (e.key === "Escape") { setNewProjectInput(false); setNewProjectName(""); }
            }}
            placeholder="Project name..."
            autoFocus
            className="w-full rounded-md bg-card px-3 py-2 text-[0.83rem] text-foreground outline-none ring-1 ring-border placeholder:text-muted-foreground/50"
          />
        </div>
      ) : (
        <button
          onClick={() => setNewProjectInput(true)}
          className="flex shrink-0 items-center gap-1 rounded-md px-[0.6rem] py-2 text-[0.8rem] text-muted-foreground hover:bg-card hover:text-foreground"
        >
          <FolderPlus size={14} />
          New project
        </button>
      )}

      {selected.size > 0 && <BulkDeleteBar count={selected.size} onDelete={bulkDelete} />}
    </>
  );
}
