import { createContext, useContext, useMemo, useState, type PointerEvent as ReactPointerEvent } from "react";
import {
  Bot,
  FolderPlus,
  GitBranch,
  RefreshCw,
  Trash2,
  ChevronRight,
  ChevronDown,
  Pencil,
  Plus,
  Settings2,
  Clapperboard,
  X,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { api } from "@/lib/tauri";
import { chatOrigin, conversationParentId, subagentsByParent } from "@/lib/transcript";
import { groupTarget, projectTarget, sidebarDrop, UNGROUPED_TARGET, type SidebarDrag } from "@/lib/sidebar-drop";
import { usePointerDrag } from "@/hooks/usePointerDrag";
import { useDesktop } from "@/store";
import { useNavigation } from "@/stores/navigation";
import { useSelection } from "@/stores/selection";
import { useAgents } from "@/stores/agents";
import { useBackend } from "@/stores/backend";
import { agentModelServed } from "@/lib/agents";
import { useSidebar } from "@/stores/sidebar";
import { useGit } from "@/stores/git";
import { useStatusActions } from "@/stores/status";
import { Button } from "@/components/ui/button";
import { AgentAvatar } from "./AgentAvatar";
import type { MarkdownAgent } from "@/lib/tauri";

const StartDragContext = createContext<(e: ReactPointerEvent, drag: SidebarDrag) => void>(() => {});

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
  const { conversations, sessionId, onChatClick, deleteConversation, isRunning, isAwaitingApproval, recordingOwner } =
    useDesktop();
  const { selected } = useSelection();
  const conv = conversations[index];
  const [confirm, setConfirm] = useState(false);
  const isSelected = selected.has(conv.id);
  const startDrag = useContext(StartDragContext);
  const isActive = conv.id === sessionId;
  const running = isRunning(conv.id);
  const awaiting = isAwaitingApproval(conv.id);
  const recording = recordingOwner === conv.id;
  const label = conversationParentId(conv) ? conv.label : null;

  return (
    <div
      role="listitem"
      title={chatOrigin(conv, conversations)}
      aria-selected={isSelected || undefined}
      onClick={(e) => onChatClick(index, e)}
      onPointerDown={(e) => {
        e.stopPropagation();
        startDrag(e, { kind: "chats", ids: isSelected ? Array.from(selected) : [conv.id] });
      }}
      className={cn(
        "group flex cursor-pointer items-center gap-2 rounded-lg px-2.5 py-1.5 text-[0.83rem] text-muted-foreground transition-colors hover:bg-foreground/5 hover:text-foreground",
        isActive && "bg-card font-medium text-foreground shadow-sm ring-1 ring-border ring-inset hover:bg-card",
        isSelected && "bg-primary/10 text-foreground ring-1 ring-primary/40 ring-inset hover:bg-primary/15",
      )}
    >
      {running && (
        <span
          aria-label={awaiting ? "Awaiting input" : recording ? "Recording" : "Running"}
          title={awaiting ? "Awaiting input" : recording ? "Recording the screen" : "Running"}
          className={cn(
            "h-2 w-2 shrink-0 rounded-full",
            awaiting ? "bg-amber-500" : recording ? "animate-pulse bg-red-500" : "animate-pulse bg-emerald-500",
          )}
        />
      )}
      {label && (
        <span className="shrink-0 rounded bg-foreground/10 px-1.5 text-[0.7rem] leading-relaxed text-muted-foreground">
          {label}
        </span>
      )}
      <span className="min-w-0 flex-1 truncate">{conv.title || "(untitled)"}</span>
      <button
        aria-label={`Delete conversation ${conv.title || conv.id}`}
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
            : "text-muted-foreground hover:bg-foreground/10 hover:text-destructive",
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
  onCleanup,
  onCancel,
  onSelectAll,
  onClear,
}: {
  count: number;
  total: number;
  onInit: () => void;
  onCleanup: () => void;
  onCancel: () => void;
  onSelectAll: () => void;
  onClear: () => void;
}) {
  const [armed, setArmed] = useState(false);
  const [cleanupArmed, setCleanupArmed] = useState(false);
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
        <Button
          variant="outline"
          disabled={count === 0}
          title="Remove all worktrees, checkout the default branch, pull and delete the local branches whose remote branch is gone"
          onClick={() => {
            if (!cleanupArmed) {
              setCleanupArmed(true);
              return;
            }
            onCleanup();
          }}
          onMouseLeave={() => setCleanupArmed(false)}
          className="text-[0.83rem]"
        >
          {cleanupArmed ? "Click again" : "Clean up"}
        </Button>
        <Button variant="outline" onClick={onCancel} className="text-[0.83rem]">
          Cancel
        </Button>
      </div>
    </div>
  );
}

// A chat row with the subagent chats it spawned indented under it.
function ChatThread({ index, subagents }: { index: number; subagents?: number[] }) {
  const { conversations } = useDesktop();
  return (
    <>
      <ChatItem index={index} />
      {subagents && (
        <div className="ml-3 flex flex-col gap-1 border-l border-border pl-2">
          {subagents.map((i) => (
            <ChatItem key={conversations[i]?.id ?? i} index={i} />
          ))}
        </div>
      )}
    </>
  );
}

// An agent row under the Agents filter: face, name, its chats nested below.
function AgentGroup({
  agent,
  count,
  collapsed,
  active,
  onToggle,
  onSelect,
  onEdit,
  onDelete,
  children,
}: {
  agent: MarkdownAgent;
  count: number;
  collapsed: boolean;
  active: boolean;
  onToggle: () => void;
  onSelect: () => void;
  onEdit: () => void;
  onDelete: () => void;
  children: React.ReactNode;
}) {
  const [armed, setArmed] = useState(false);
  const { models } = useBackend();
  const served = agentModelServed(agent, models);
  return (
    <div className="group/agent">
      <div
        onClick={onSelect}
        title={agent.description}
        className={cn(
          "flex cursor-pointer items-center gap-1.5 rounded-lg px-2 py-1.5 text-[0.8rem] font-semibold text-muted-foreground transition-colors hover:bg-foreground/5 hover:text-foreground",
          active && "bg-primary/10 text-foreground ring-1 ring-primary/25 ring-inset hover:bg-primary/15",
        )}
      >
        <button
          onClick={(e) => {
            e.stopPropagation();
            onToggle();
          }}
          aria-label={collapsed ? `Expand ${agent.name}` : `Collapse ${agent.name}`}
          className="inline-flex shrink-0 items-center"
        >
          {collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
        </button>
        <AgentAvatar agent={agent} size={22} />
        <div className="flex min-w-0 flex-col">
          <button
            aria-label={`Select agent ${agent.name}`}
            onClick={(e) => {
              e.stopPropagation();
              onSelect();
            }}
            className="min-w-0 self-start truncate"
          >
            {agent.name}
          </button>
          {agent.email.trim() && (
            <span
              title={agent.email}
              className="max-w-full truncate text-[0.62rem] font-normal text-muted-foreground/60"
            >
              {agent.email}
            </span>
          )}
        </div>
        <span className="text-[0.7rem] text-muted-foreground/60">{`(${count})`}</span>
        <span
          title={
            served
              ? agent.model || "Runs on the model selected in the top bar"
              : `${agent.model} is not served by this computer's gateway, so a chat or subagent here cannot run it - edit the agent to pick another model or inherit the selected one`
          }
          className={cn(
            "min-w-0 truncate text-[0.7rem] font-normal",
            served ? "text-muted-foreground/60" : "text-warn",
          )}
        >
          {agent.model.split("/").pop() || "selected model"}
          {!served && " - not served"}
        </span>
        <div className="ml-auto flex items-center gap-0.5 opacity-0 group-hover/agent:opacity-100 focus-within:opacity-100">
          <button
            aria-label={`Edit agent ${agent.name}`}
            title="Edit agent"
            onClick={(e) => {
              e.stopPropagation();
              onEdit();
            }}
            className="inline-flex items-center justify-center rounded p-[0.15rem] text-muted-foreground hover:text-foreground"
          >
            <Pencil size={11} />
          </button>
          <button
            aria-label={`Delete agent ${agent.name}`}
            title={armed ? "Click again to delete" : "Delete agent"}
            onClick={(e) => {
              e.stopPropagation();
              if (armed) onDelete();
              setArmed(!armed);
            }}
            onBlur={() => setArmed(false)}
            className={cn(
              "inline-flex items-center justify-center rounded p-[0.15rem] text-muted-foreground hover:text-destructive",
              armed && "text-destructive",
            )}
          >
            <X size={11} />
          </button>
        </div>
      </div>
      {!collapsed && count > 0 && (
        <div className="mt-1 mb-2 ml-3 flex flex-col gap-1 border-l border-border pl-2">{children}</div>
      )}
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
  isContent,
  dirty,
  branch,
  defaultBranch,
  remote,
  busy,
  collapsed,
  active,
  onToggle,
  onSelect,
  onInit,
  onOpenInVsCode,
  onOpenOnGithub,
  onSyncDefaultBranch,
  onCleanup,
  onSettings,
  onRename,
  onDelete,
  onStartDrag,
  dropOver,
  children,
}: {
  name: string;
  selecting: boolean;
  checked: boolean;
  onToggleCheck: () => void;
  count: number;
  isGit: boolean;
  isContent: boolean;
  dirty: boolean;
  branch?: string;
  defaultBranch?: string;
  remote?: string;
  busy: boolean;
  collapsed: boolean;
  active: boolean;
  onToggle: () => void;
  onSelect: () => void;
  onInit: () => void;
  onOpenInVsCode: () => void;
  onOpenOnGithub?: () => void;
  onSyncDefaultBranch: () => void;
  onCleanup: () => void;
  onSettings: () => void;
  onRename: (newName: string) => void;
  onDelete: () => void;
  onStartDrag: (e: ReactPointerEvent) => void;
  dropOver: boolean;
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
      data-drop-target={projectTarget(name)}
      onPointerDown={(e) => {
        if (!editing) onStartDrag(e);
      }}
      className={cn("group/project rounded-lg", dropOver && "ring-1 ring-primary/30")}
    >
      <div
        onClick={onSelect}
        onContextMenu={openMenu}
        className={cn(
          "flex cursor-pointer items-center gap-1 rounded-lg px-2 py-1.5 text-[0.8rem] font-semibold text-muted-foreground transition-colors hover:bg-foreground/5 hover:text-foreground",
          active && "bg-primary/10 text-foreground ring-1 ring-primary/25 ring-inset hover:bg-primary/15",
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
            <button
              aria-label={`Select project ${name}`}
              onClick={(e) => {
                e.stopPropagation();
                onSelect();
              }}
              onDoubleClick={(e) => {
                e.stopPropagation();
                setEditing(true);
                setEditValue(name);
              }}
              className="truncate"
            >
              {name}
            </button>
          )}
          <span className="ml-1 text-[0.7rem] text-muted-foreground/60">{`(${count})`}</span>
          {isContent && <Clapperboard size={11} className="ml-1 shrink-0 text-muted-foreground/60" />}
          {isGit &&
            (remote && onOpenOnGithub ? (
              <button
                aria-label={`Open project ${name} on GitHub`}
                title="Open on GitHub"
                onClick={(e) => {
                  e.stopPropagation();
                  onOpenOnGithub();
                }}
                className="inline-flex shrink-0 items-center"
              >
                <GitBranch
                  size={11}
                  className={cn("ml-0.5 shrink-0", dirty ? "text-amber-500" : "text-muted-foreground/60")}
                />
              </button>
            ) : (
              <GitBranch
                size={11}
                aria-label="Git repository"
                className={cn("ml-0.5 shrink-0", dirty ? "text-amber-500" : "text-muted-foreground/60")}
              />
            ))}
          {isGit && branch && (
            <span title={branch} className="max-w-28 truncate text-[0.65rem] font-normal text-muted-foreground/60">
              {branch}
            </span>
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
      {!collapsed && count > 0 && (
        <div className="mt-1 mb-2 ml-3 flex flex-col gap-1 border-l border-border pl-2">{children}</div>
      )}
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
            style={{ left: menu.x, top: Math.min(menu.y, window.innerHeight - 160) }}
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
            {isGit && remote && onOpenOnGithub && (
              <button
                aria-label={`Open project ${name} on GitHub`}
                title={`Open https://github.com/${remote} in your browser`}
                onClick={() => {
                  setMenu(null);
                  onOpenOnGithub();
                }}
                className="w-full rounded px-2 py-1.5 text-left text-[0.8rem] text-foreground hover:bg-background"
              >
                Open on GitHub
              </button>
            )}
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
            {isGit && branch && defaultBranch && branch !== defaultBranch && (
              <button
                aria-label={`Sync project ${name} to default branch`}
                title={`Checkout ${defaultBranch} and pull the latest changes`}
                disabled={busy || !dirOk}
                onClick={() => {
                  setMenu(null);
                  onSyncDefaultBranch();
                }}
                className="w-full rounded px-2 py-1.5 text-left text-[0.8rem] text-foreground hover:bg-background disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent"
              >
                Checkout default branch + pull
              </button>
            )}
            {isGit && (
              <button
                aria-label={`Clean up branches in project ${name}`}
                title="Remove all worktrees, checkout the default branch, pull and delete the local branches whose remote branch is gone"
                disabled={busy || !dirOk}
                onClick={() => {
                  setMenu(null);
                  onCleanup();
                }}
                className="w-full rounded px-2 py-1.5 text-left text-[0.8rem] text-foreground hover:bg-background disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent"
              >
                Clean up branches
              </button>
            )}
          </div>
        </>
      )}
    </div>
  );
}

export function ChatList() {
  const {
    gitProjects,
    dirtyProjects,
    projectBranches,
    projectDefaultBranches,
    projectRemotes,
    refreshGitProjects,
    refreshProjects,
    cleanupProject,
    syncDefaultBranch,
  } = useGit();
  const {
    projects,
    projectNames,
    collapsedProjects,
    deleteProject,
    renameProject,
    toggleCollapseProject,
    createProject,
    moveProject,
    initSelection,
    activeProject,
    projectGroups,
    projectTypes,
    agentOf,
  } = useSidebar();
  const { agents, activeAgent, deleteAgent, loadAgents } = useAgents();
  const {
    conversations,
    bulkDelete,
    assignProject,
    unassignProject,
    initProject,
    initAllProjects,
    cleanupProjects,
    initSelecting,
    toggleInitSelection,
    cancelInitSelection,
    selectAllProjects,
    clearProjectSelection,
    selectProjectsInGroup,
    isRunning,
    selectProject,
    visibleProjectNames,
    selectAgent,
    openAgentEditor,
  } = useDesktop();
  const { setInitialSettingsTab, setInitialProjectFilter, setCurrentView, projectFilter: filter } = useNavigation();
  const { selected } = useSelection();
  const { setError } = useStatusActions();
  const agentsFilter = filter === "agents";
  const [collapsedAgents, setCollapsedAgents] = useState<Set<string>>(() => new Set());

  const [newProjectInput, setNewProjectInput] = useState(false);
  const [newProjectName, setNewProjectName] = useState("");
  const [newProjectGroup, setNewProjectGroup] = useState("");

  const groups = useMemo(() => {
    const map: Record<string, number[]> = {};
    for (const name of projectNames) {
      map[name] = [];
    }
    const ungrouped: number[] = [];
    const subagents = subagentsByParent(conversations);
    const nested = new Set(Object.values(subagents).flat());
    conversations.forEach((conv, i) => {
      if (nested.has(i)) return;
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
      subagents,
    };
  }, [conversations, projects, projectNames]);

  const chatThread = (i: number) => (
    <ChatThread key={conversations[i]?.id ?? i} index={i} subagents={groups.subagents[conversations[i]?.id ?? ""]} />
  );

  const drag = usePointerDrag<SidebarDrag>((dragged, target) => {
    const drop = sidebarDrop(dragged, target);
    if (drop?.kind === "assign") for (const id of drop.ids) assignProject(id, drop.project);
    if (drop?.kind === "unassign") for (const id of drop.ids) unassignProject(id);
    if (drop?.kind !== "moveProject" || (projectGroups[drop.name] ?? "") === drop.group) return;
    if (projectBusy(drop.name)) {
      setError(`${drop.name} is running - stop it before moving`);
      return;
    }
    moveProject(drop.name, drop.group);
  });

  const handleNewProject = () => {
    const trimmed = newProjectName.trim();
    if (trimmed) {
      createProject(trimmed, newProjectGroup || undefined);
      setNewProjectInput(false);
      setNewProjectName("");
      setNewProjectGroup("");
    }
  };

  /** A run in flight anywhere in the project (an init included) blocks another. */
  const projectBusy = (name: string) => conversations.some((c) => projects[c.id] === name && isRunning(c.id));

  const projectClusters = useMemo(() => {
    const clusters = new Map<string, [string, number[]][]>();
    for (const entry of groups.projects) {
      if (filter && (projectTypes[entry[0]] ?? "code") !== filter) continue;
      const label = projectGroups[entry[0]] ?? "";
      (clusters.get(label) ?? clusters.set(label, []).get(label)!).push(entry);
    }
    return Array.from(clusters.entries()).sort(([a], [b]) => a.localeCompare(b));
  }, [groups.projects, projectGroups, projectTypes, filter]);

  const agentChats = useMemo(() => {
    const map: Record<string, number[]> = {};
    conversations.forEach((conv, i) => {
      const name = agentOf[conv.id];
      if (name) (map[name] ??= []).push(i);
    });
    return map;
  }, [conversations, agentOf]);

  const agentRows = (
    <>
      {agents.map((agent) => (
        <AgentGroup
          key={agent.name}
          agent={agent}
          count={agentChats[agent.name]?.length ?? 0}
          collapsed={collapsedAgents.has(agent.name)}
          active={activeAgent === agent.name}
          onToggle={() =>
            setCollapsedAgents((prev) => {
              const next = new Set(prev);
              if (next.has(agent.name)) next.delete(agent.name);
              else next.add(agent.name);
              return next;
            })
          }
          onSelect={() => selectAgent(activeAgent === agent.name ? null : agent.name)}
          onEdit={() => openAgentEditor(agent)}
          onDelete={() => deleteAgent(agent.name).catch((e) => setError(String(e)))}
        >
          {(agentChats[agent.name] ?? []).map((i) => (
            <ChatItem key={conversations[i]?.id ?? i} index={i} />
          ))}
        </AgentGroup>
      ))}
      {agents.length === 0 && (
        <p className="px-[0.6rem] py-4 text-center text-[0.8rem] text-muted-foreground">
          No agents yet. Create one with + New agent.
        </p>
      )}
    </>
  );

  const orchestratorRows = (
    <>
      {projectClusters.map(([label, entries]) => (
        <div key={label || "(ungrouped)"} className="flex flex-col gap-1">
          {label && (
            <div
              data-drop-target={groupTarget(label)}
              className={cn(
                "mt-1 flex items-center justify-between rounded-md px-[0.4rem] text-[0.68rem] font-medium tracking-wide text-muted-foreground/50 uppercase",
                drag.over === groupTarget(label) && "ring-1 ring-primary/30",
              )}
            >
              <span className="flex items-center gap-1">
                {label}
                <button
                  onClick={() => {
                    setNewProjectGroup(label);
                    setNewProjectInput(true);
                  }}
                  aria-label={`New project in ${label}`}
                  title={`New project in ${label}`}
                  className="rounded p-0.5 hover:text-foreground"
                >
                  <Plus size={11} />
                </button>
              </span>
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
                isGit={gitProjects.has(name) && projectTypes[name] !== "content"}
                isContent={projectTypes[name] === "content"}
                dirty={dirtyProjects.has(name)}
                branch={projectBranches[name]}
                defaultBranch={projectDefaultBranches[name]}
                remote={projectRemotes[name]}
                busy={projectBusy(name)}
                collapsed={collapsed}
                active={activeProject === name}
                onToggle={() => toggleCollapseProject(name)}
                onSelect={() => selectProject(activeProject === name ? null : name)}
                onInit={() => initProject(name)}
                onOpenInVsCode={() => api.openInVsCode(name).catch((e) => setError(String(e)))}
                onOpenOnGithub={
                  projectRemotes[name]
                    ? () => api.openUrl(`https://github.com/${projectRemotes[name]}`).catch((e) => setError(String(e)))
                    : undefined
                }
                onSyncDefaultBranch={() => syncDefaultBranch(name).then(refreshGitProjects)}
                onCleanup={() => cleanupProject(name).then(refreshGitProjects)}
                onSettings={() => {
                  setInitialSettingsTab("projects");
                  setInitialProjectFilter(name);
                  setCurrentView("settings");
                }}
                onRename={(n) => renameProject(name, n)}
                onDelete={() => deleteProject(name)}
                onStartDrag={(e) => drag.start(e, { kind: "project", name })}
                dropOver={drag.over === projectTarget(name)}
              >
                {indices.map(chatThread)}
              </ProjectGroup>
            );
          })}
        </div>
      ))}

      {groups.ungrouped.length > 0 && (
        <div
          data-drop-target={UNGROUPED_TARGET}
          className={cn("flex flex-col gap-1 rounded-md", drag.over === UNGROUPED_TARGET && "ring-1 ring-primary/30")}
        >
          {groups.ungrouped.map(chatThread)}
        </div>
      )}
    </>
  );

  return (
    <StartDragContext.Provider value={drag.start}>
      <div id="chat-list" role="list" className="flex flex-1 flex-col gap-1 overflow-y-auto select-none">
        {agentsFilter ? agentRows : orchestratorRows}

        {!agentsFilter && conversations.length === 0 && (
          <p className="px-[0.6rem] py-4 text-center text-[0.8rem] text-muted-foreground">No conversations yet</p>
        )}
      </div>

      {agentsFilter ? (
        <div className="flex shrink-0 items-center border-t border-border">
          <button
            onClick={() => openAgentEditor(null)}
            className="flex flex-1 items-center gap-1 rounded-md px-[0.6rem] py-2 text-[0.8rem] text-muted-foreground hover:bg-card hover:text-foreground"
          >
            <Bot size={14} />
            <Plus size={11} />
            New agent
          </button>
          <button
            aria-label="Refresh agents"
            title="Reload agents from disk"
            onClick={loadAgents}
            className="mr-1.5 inline-flex shrink-0 items-center justify-center rounded-md p-1.5 text-muted-foreground hover:bg-card hover:text-foreground"
          >
            <RefreshCw size={14} />
          </button>
        </div>
      ) : newProjectInput ? (
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
                setNewProjectGroup("");
              }
            }}
            placeholder={newProjectGroup ? `Project name in ${newProjectGroup}...` : "Project name..."}
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
            onClick={refreshProjects}
            className="mr-1.5 inline-flex shrink-0 items-center justify-center rounded-md p-1.5 text-muted-foreground hover:bg-card hover:text-foreground"
          >
            <RefreshCw size={14} />
          </button>
        </div>
      )}

      {initSelecting && (
        <InitBar
          count={initSelection.size}
          total={visibleProjectNames.length}
          onInit={() => {
            initAllProjects(Array.from(initSelection));
            cancelInitSelection();
          }}
          onCleanup={() => {
            cleanupProjects(Array.from(initSelection));
            cancelInitSelection();
          }}
          onCancel={cancelInitSelection}
          onSelectAll={selectAllProjects}
          onClear={clearProjectSelection}
        />
      )}
      {selected.size > 0 && <BulkDeleteBar count={selected.size} onDelete={bulkDelete} />}
    </StartDragContext.Provider>
  );
}
