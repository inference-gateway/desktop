import { useMemo, useState, type DragEvent } from "react";
import { FolderPlus, GitBranch, Trash2, ChevronRight, ChevronDown, Pencil, Settings2, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { subagentParentId } from "@/lib/transcript";
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
  const parentId = subagentParentId(conv.id);
  const parentTitle = parentId
    ? conversations.find((c) => c.id === parentId)?.title || parentId.slice(0, 5)
    : null;

  return (
    <div
      role="listitem"
      title={
        parentTitle
          ? `${conv.title || conv.id} - invoked by orchestrator: ${parentTitle}`
          : `${conv.title || conv.id} - invoked by human`
      }
      aria-selected={isSelected || undefined}
      onClick={(e) => onChatClick(index, e)}
      draggable
      onDragStart={(e) => {
        const ids = isSelected ? Array.from(selected) : [conv.id];
        e.dataTransfer.setData("text/plain", JSON.stringify(ids));
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

function Section({
  title,
  count,
  collapsed,
  onToggle,
  className,
  children,
}: {
  title: string;
  count: number;
  collapsed: boolean;
  onToggle: () => void;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <div className={className}>
      <button
        onClick={onToggle}
        aria-label={collapsed ? `Expand ${title}` : `Collapse ${title}`}
        className="flex w-full items-center gap-1 border-b border-border/60 px-[0.4rem] pb-1.5 pt-1 text-[0.82rem] font-semibold text-foreground/90"
      >
        {collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
        {title}
        <span className="ml-1 text-[0.72rem] font-normal text-muted-foreground/60">({count})</span>
      </button>
      {!collapsed && <div className="mt-1 flex flex-col gap-[2px] pl-3">{children}</div>}
    </div>
  );
}

function ProjectGroup({
  name,
  count,
  isGit,
  collapsed,
  active,
  onToggle,
  onSelect,
  onSettings,
  onRename,
  onDelete,
  onDrop,
  onDragOverProject,
  children,
}: {
  name: string;
  count: number;
  isGit: boolean;
  collapsed: boolean;
  active: boolean;
  onToggle: () => void;
  onSelect: () => void;
  onSettings: () => void;
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
        onClick={onSelect}
        aria-label={`Select project ${name}`}
        className={cn(
          "flex cursor-pointer items-center gap-1 rounded-md px-[0.4rem] py-1.5 text-[0.8rem] font-semibold text-muted-foreground hover:bg-card",
          active && "bg-primary/10 text-foreground shadow-[inset_3px_0_0_var(--primary)]",
        )}
      >
        <button
          onClick={(e) => { e.stopPropagation(); onToggle(); }}
          aria-label={collapsed ? `Expand ${name}` : `Collapse ${name}`}
          className="inline-flex shrink-0 items-center"
        >
          {collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
        </button>
        <span className="inline-flex min-w-0 items-center gap-1 truncate">
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
          {isGit && (
            <GitBranch
              size={11}
              aria-label="Git repository"
              className="ml-0.5 shrink-0 text-muted-foreground/60"
            />
          )}
        </span>
        <div className="ml-auto flex items-center gap-0.5 opacity-0 group-hover/project:opacity-100 focus-within:opacity-100">
          <button
            aria-label={`Settings for project ${name}`}
            title="Project settings"
            onClick={(e) => { e.stopPropagation(); onSettings(); }}
            className="inline-flex items-center justify-center rounded p-[0.15rem] text-muted-foreground hover:text-foreground"
          >
            <Settings2 size={11} />
          </button>
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
    activeProject,
    setActiveProject,
    setInitialSettingsTab,
    setCurrentView,
    gitProjects,
  } = useDesktop();

  const [newProjectInput, setNewProjectInput] = useState(false);
  const [newProjectName, setNewProjectName] = useState("");
  const [dragOverUngrouped, setDragOverUngrouped] = useState(false);
  const [orchestratorsCollapsed, setOrchestratorsCollapsed] = useState(false);
  const [agentsCollapsed, setAgentsCollapsed] = useState(false);

  const groups = useMemo(() => {
    const map: Record<string, number[]> = {};
    for (const name of projectNames) {
      map[name] = [];
    }
    const ungrouped: number[] = [];
    const agents: number[] = [];
    conversations.forEach((conv, i) => {
      if (subagentParentId(conv.id)) {
        agents.push(i);
        return;
      }
      const p = projects[conv.id];
      if (p && map[p]) {
        map[p].push(i);
      } else {
        ungrouped.push(i);
      }
    });
    return {
      projects: Object.entries(map),
      ungrouped,
      agents,
    };
  }, [conversations, projects, projectNames]);

  const draggedIds = (e: DragEvent): string[] => {
    try {
      const parsed = JSON.parse(e.dataTransfer.getData("text/plain"));
      return Array.isArray(parsed) ? parsed.filter((id) => typeof id === "string") : [];
    } catch {
      return [];
    }
  };

  const handleDropOnProject = (e: DragEvent, projectName: string) => {
    e.preventDefault();
    for (const id of draggedIds(e)) assignProject(id, projectName);
  };

  const handleDropOnUngrouped = (e: DragEvent) => {
    e.preventDefault();
    for (const id of draggedIds(e)) unassignProject(id);
  };

  const handleNewProject = () => {
    const trimmed = newProjectName.trim();
    if (trimmed) {
      createProject(trimmed);
      setNewProjectInput(false);
      setNewProjectName("");
    }
  };

  const orchestratorRows = (
    <>
      {groups.projects.map(([name, indices]) => {
        const collapsed = collapsedProjects.has(name);
        return (
          <ProjectGroup
            key={name}
            name={name}
            count={indices.length}
            isGit={gitProjects.has(name)}
            collapsed={collapsed}
            active={activeProject === name}
            onToggle={() => toggleCollapseProject(name)}
            onSelect={() => setActiveProject(activeProject === name ? null : name)}
            onSettings={() => { setInitialSettingsTab("projects"); setCurrentView("settings"); }}
            onRename={(n) => renameProject(name, n)}
            onDelete={() => deleteProject(name)}
            onDrop={(e) => handleDropOnProject(e, name)}
            onDragOverProject={() => {}}
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
    </>
  );

  return (
    <>
      <div id="chat-list" role="list" className="flex flex-1 flex-col gap-[2px] overflow-y-auto">
        {groups.agents.length > 0 ? (
          <>
            <Section
              title="Orchestrators"
              count={conversations.length - groups.agents.length}
              collapsed={orchestratorsCollapsed}
              onToggle={() => setOrchestratorsCollapsed((v) => !v)}
            >
              {orchestratorRows}
            </Section>
            <Section
              title="Agents"
              count={groups.agents.length}
              collapsed={agentsCollapsed}
              onToggle={() => setAgentsCollapsed((v) => !v)}
              className="mt-4"
            >
              {groups.agents.map((i) => (
                <ChatItem key={conversations[i]?.id ?? i} index={i} />
              ))}
            </Section>
          </>
        ) : (
          orchestratorRows
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
