import { useMemo, useState, type DragEvent } from "react";
import {
  FolderPlus,
  GitBranch,
  RefreshCw,
  Trash2,
  ChevronRight,
  ChevronDown,
  Pencil,
  Settings2,
  X,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { api } from "@/lib/tauri";
import { subagentParentId } from "@/lib/transcript";
import { useDesktop } from "@/store";
import { Button } from "@/components/ui/button";

function CheckboxGlyph({ checked }: { checked: boolean }) {
  return (
    <span
      aria-hidden
      className={cn(
        "inline-block size-3.5 shrink-0 rounded-[4px] border",
        checked ? "border-primary bg-primary" : "border-muted-foreground/50",
      )}
    />
  );
}

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
  const parentTitle = parentId ? conversations.find((c) => c.id === parentId)?.title || parentId.slice(0, 5) : null;

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
        isSelected && "bg-primary/10 text-foreground shadow-[inset_3px_0_0_var(--primary)] ring-1 ring-primary/25",
      )}
    >
      {running && (
        <span
          aria-label={awaiting ? "Awaiting approval" : "Running"}
          title={awaiting ? "Awaiting approval" : "Running"}
          className={cn("h-2 w-2 shrink-0 rounded-full", awaiting ? "bg-amber-500" : "animate-pulse bg-emerald-500")}
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
          confirm
            ? "bg-destructive text-white opacity-100"
            : "text-muted-foreground hover:bg-card hover:text-destructive",
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

function InitBar({
  count,
  total,
  onInit,
  onCancel,
  onSelectAll,
  onClear,
}: {
  count: number;
  total: number;
  onInit: () => void;
  onCancel: () => void;
  onSelectAll: () => void;
  onClear: () => void;
}) {
  const [armed, setArmed] = useState(false);
  const allSelected = total > 0 && count >= total;
  return (
    <div className="flex shrink-0 flex-col gap-2 border-t border-border py-2">
      <div className="flex items-center justify-between px-0.5 text-[0.78rem] text-muted-foreground">
        <span>{count} selected</span>
        <button
          onClick={allSelected ? onClear : onSelectAll}
          aria-label={allSelected ? "Deselect all projects" : "Select all projects"}
          className="rounded px-1 py-0.5 hover:text-foreground"
        >
          {allSelected ? "Deselect all" : "Select all"}
        </button>
      </div>
      <div className="flex items-center gap-2">
        <Button
          disabled={count === 0}
          onClick={() => {
            if (!armed) {
              setArmed(true);
              return;
            }
            onInit();
          }}
          onMouseLeave={() => setArmed(false)}
          className="flex-1 text-[0.83rem]"
        >
          {count === 0
            ? "Select projects to init"
            : armed
              ? `Click again to init ${count}`
              : `Init ${count} project${count === 1 ? "" : "s"}`}
        </Button>
        <Button variant="outline" onClick={onCancel} className="text-[0.83rem]">
          Cancel
        </Button>
      </div>
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
  selecting,
  checked,
  onToggleCheck,
  count,
  isGit,
  dirty,
  busy,
  collapsed,
  active,
  onToggle,
  onSelect,
  onInit,
  onOpenInVsCode,
  onSettings,
  onRename,
  onDelete,
  onDrop,
  onDragOverProject,
  children,
}: {
  name: string;
  selecting: boolean;
  checked: boolean;
  onToggleCheck: () => void;
  count: number;
  isGit: boolean;
  dirty: boolean;
  busy: boolean;
  collapsed: boolean;
  active: boolean;
  onToggle: () => void;
  onSelect: () => void;
  onInit: () => void;
  onOpenInVsCode: () => void;
  onSettings: () => void;
  onRename: (newName: string) => void;
  onDelete: () => void;
  onDrop: (e: DragEvent) => void;
  onDragOverProject: (over: boolean) => void;
  children: React.ReactNode;
}) {
  const [editing, setEditing] = useState(false);
  const [editValue, setEditValue] = useState(name);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [dirOk, setDirOk] = useState(false);

  const openMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    setDirOk(false);
    setMenu({ x: e.clientX, y: e.clientY });
    api
      .projectDirExists(name)
      .then(setDirOk)
      .catch(() => {});
  };

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
        onContextMenu={openMenu}
        aria-label={`Select project ${name}`}
        className={cn(
          "flex cursor-pointer items-center gap-1 rounded-md px-[0.4rem] py-1.5 text-[0.8rem] font-semibold text-muted-foreground hover:bg-card",
          active && "bg-primary/10 text-foreground shadow-[inset_3px_0_0_var(--primary)]",
        )}
      >
        {selecting && (
          <button
            aria-label={`Toggle init for project ${name}`}
            aria-pressed={checked}
            onClick={(e) => {
              e.stopPropagation();
              onToggleCheck();
            }}
            className="inline-flex shrink-0 items-center"
          >
            <CheckboxGlyph checked={checked} />
          </button>
        )}
        <button
          onClick={(e) => {
            e.stopPropagation();
            onToggle();
          }}
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
            <span
              onDoubleClick={(e) => {
                e.stopPropagation();
                setEditing(true);
                setEditValue(name);
              }}
            >
              {name}
            </span>
          )}
          <span className="ml-1 text-[0.7rem] text-muted-foreground/60">({count})</span>
          {isGit && (
            <GitBranch
              size={11}
              aria-label="Git repository"
              className={cn("ml-0.5 shrink-0", dirty ? "text-amber-500" : "text-muted-foreground/60")}
            />
          )}
        </span>
        <div className="ml-auto flex items-center gap-0.5 opacity-0 group-hover/project:opacity-100 focus-within:opacity-100">
          <button
            aria-label={`Settings for project ${name}`}
            title="Project settings"
            onClick={(e) => {
              e.stopPropagation();
              onSettings();
            }}
            className="inline-flex items-center justify-center rounded p-[0.15rem] text-muted-foreground hover:text-foreground"
          >
            <Settings2 size={11} />
          </button>
          <button
            aria-label={`Rename project ${name}`}
            title="Rename"
            onClick={(e) => {
              e.stopPropagation();
              setEditing(true);
              setEditValue(name);
            }}
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
      {menu && (
        <>
          <div
            className="fixed inset-0 z-40"
            onClick={() => setMenu(null)}
            onContextMenu={(e) => {
              e.preventDefault();
              setMenu(null);
            }}
          />
          <div
            style={{ left: menu.x, top: Math.min(menu.y, window.innerHeight - 84) }}
            className="fixed z-50 min-w-36 rounded-md border border-border bg-card p-1 shadow-md"
          >
            <button
              aria-label={`Open project ${name} in VS Code`}
              title="Open the project folder in VS Code"
              disabled={!dirOk}
              onClick={() => {
                setMenu(null);
                onOpenInVsCode();
              }}
              className="w-full rounded px-2 py-1.5 text-left text-[0.8rem] text-foreground hover:bg-background disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent"
            >
              Open in VS Code
            </button>
            <button
              aria-label={`Init project ${name}`}
              title="Run /init to create or update AGENTS.md"
              disabled={busy || !dirOk}
              onClick={() => {
                setMenu(null);
                onInit();
              }}
              className="w-full rounded px-2 py-1.5 text-left text-[0.8rem] text-foreground hover:bg-background disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent"
            >
              Init
            </button>
          </div>
        </>
      )}
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
    initProject,
    initAllProjects,
    initSelecting,
    initSelection,
    toggleInitSelection,
    cancelInitSelection,
    selectAllProjects,
    clearProjectSelection,
    selectProjectsInGroup,
    isRunning,
    activeProject,
    setActiveProject,
    setInitialSettingsTab,
    setInitialProjectFilter,
    setCurrentView,
    gitProjects,
    dirtyProjects,
    refreshGitProjects,
    projectGroups,
    setError,
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

  // A run in flight anywhere in the project (an init included) blocks another.
  const projectBusy = (name: string) => conversations.some((c) => projects[c.id] === name && isRunning(c.id));

  const projectClusters = useMemo(() => {
    const clusters = new Map<string, [string, number[]][]>();
    for (const entry of groups.projects) {
      const label = projectGroups[entry[0]] ?? "";
      (clusters.get(label) ?? clusters.set(label, []).get(label)!).push(entry);
    }
    return Array.from(clusters.entries()).sort(([a], [b]) => a.localeCompare(b));
  }, [groups.projects, projectGroups]);

  const orchestratorRows = (
    <>
      {projectClusters.map(([label, entries]) => (
        <div key={label || "(ungrouped)"} className="flex flex-col gap-[2px]">
          {label && (
            <div className="mt-1 flex items-center justify-between px-[0.4rem] text-[0.68rem] font-medium tracking-wide text-muted-foreground/50 uppercase">
              <span>{label}</span>
              {initSelecting && (
                <button
                  onClick={() => selectProjectsInGroup(label)}
                  aria-label={`Toggle selection for group ${label}`}
                  className="rounded px-1 normal-case hover:text-foreground"
                >
                  Select
                </button>
              )}
            </div>
          )}
          {entries.map(([name, indices]) => {
            const collapsed = collapsedProjects.has(name);
            return (
              <ProjectGroup
                key={name}
                name={name}
                selecting={initSelecting}
                checked={initSelection.has(name)}
                onToggleCheck={() => toggleInitSelection(name)}
                count={indices.length}
                isGit={gitProjects.has(name)}
                dirty={dirtyProjects.has(name)}
                busy={projectBusy(name)}
                collapsed={collapsed}
                active={activeProject === name}
                onToggle={() => toggleCollapseProject(name)}
                onSelect={() => setActiveProject(activeProject === name ? null : name)}
                onInit={() => initProject(name)}
                onOpenInVsCode={() => api.openInVsCode(name).catch((e) => setError(String(e)))}
                onSettings={() => {
                  setInitialSettingsTab("projects");
                  setInitialProjectFilter(name);
                  setCurrentView("settings");
                }}
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
        </div>
      ))}

      {groups.ungrouped.length > 0 && (
        <div
          onDragOver={(e) => {
            e.preventDefault();
            e.dataTransfer.dropEffect = "move";
            setDragOverUngrouped(true);
          }}
          onDragLeave={() => setDragOverUngrouped(false)}
          onDrop={handleDropOnUngrouped}
          className={cn("flex flex-col gap-[2px] rounded-md", dragOverUngrouped && "ring-1 ring-primary/30")}
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
      <div id="chat-list" role="list" className="flex flex-1 flex-col gap-[2px] overflow-y-auto select-none">
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
          <p className="px-[0.6rem] py-4 text-center text-[0.8rem] text-muted-foreground">No conversations yet</p>
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
              if (e.key === "Escape") {
                setNewProjectInput(false);
                setNewProjectName("");
              }
            }}
            placeholder="Project name..."
            autoFocus
            className="w-full rounded-md bg-card px-3 py-2 text-[0.83rem] text-foreground outline-none ring-1 ring-border placeholder:text-muted-foreground/50"
          />
        </div>
      ) : (
        <div className="flex shrink-0 items-center border-t border-border">
          <button
            onClick={() => setNewProjectInput(true)}
            className="flex flex-1 items-center gap-1 rounded-md px-[0.6rem] py-2 text-[0.8rem] text-muted-foreground hover:bg-card hover:text-foreground"
          >
            <FolderPlus size={14} />
            New project
          </button>
          <button
            aria-label="Refresh projects"
            title="Refresh git status for all projects"
            onClick={refreshGitProjects}
            className="mr-1.5 inline-flex shrink-0 items-center justify-center rounded-md p-1.5 text-muted-foreground hover:bg-card hover:text-foreground"
          >
            <RefreshCw size={14} />
          </button>
        </div>
      )}

      {initSelecting && (
        <InitBar
          count={initSelection.size}
          total={projectNames.length}
          onInit={() => {
            initAllProjects(Array.from(initSelection));
            cancelInitSelection();
          }}
          onCancel={cancelInitSelection}
          onSelectAll={selectAllProjects}
          onClear={clearProjectSelection}
        />
      )}
      {selected.size > 0 && <BulkDeleteBar count={selected.size} onDelete={bulkDelete} />}
    </>
  );
}
