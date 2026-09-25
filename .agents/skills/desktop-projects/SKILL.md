---
name: desktop-projects
description: Organise the Inference Gateway Desktop sidebar - its projects, groups, project directories and which chats belong to which project - by editing ~/.infer/projects.yaml and the projects.root key in ~/.infer/config.yaml. Use whenever the user wants to organise/organize, group, regroup, rename, move or import projects, change where projects live (the projects root, a folder like ~/Repositories), assign or tidy chats/conversations into projects, switch a project between code and content, or open a project in the sidebar (e.g. while recording a demo), even if they do not say "project" explicitly.
license: Apache-2.0
---

# Desktop Projects

The desktop sidebar is a view over two files owned by the user, not by the app's source code:

- `~/.infer/projects.yaml` - the projects, their groups, types, directory overrides and which chat belongs where.
- `~/.infer/config.yaml` - the `projects.root` key, the folder every project directory is derived from.

You organise projects by editing those two files and renaming directories on disk. The sidebar reloads
`projects.yaml` after every run, so the result shows up as soon as you finish. Never edit the desktop's
source code for this.

## Tools you may use

`Bash` for `infer conversations list --all-projects --format json`, `ls`, `mv`, `mkdir -p`, `git status --porcelain`,
`git remote get-url origin` and `cat`; `Read` and `Write` (or `Edit`) for the two files above, by absolute path:
the file tools do not expand `~`, so `~/.infer/projects.yaml` is refused - use `/Users/<you>/.infer/projects.yaml`
(`echo $HOME` if unsure); `Computer` only to open a project in the sidebar (below). `projects.yaml` is
carved out of the file sandbox so you can edit it directly; `config.yaml` is not, so the desktop may ask the
user to approve that write: that is expected, do not work around it. Do not use `find` over the whole home directory, package managers,
`WebFetch` or `WebSearch`.

## Where projects live

`projects.root` (in `config.yaml`, under `projects:`) defaults to `<Documents>/Inference Gateway Desktop`.
Write it as an absolute path (`/Users/me/Repositories`, not `~/Repositories`): the sandbox grant and directory
mapping use the raw value.

A project's directory is derived from its name and group:

- no group: `<root>/<sanitized name>`
- group `core`: `<root>/core/<sanitized leaf>`, where the leaf is the name minus a leading `core/` prefix (so
  a project named `core/cli` in group `core` maps to `<root>/core/cli`).

Sanitizing keeps letters, digits, space, `-`, `_` and `.`; every other character becomes `-`; leading and trailing
`-`, `.` and spaces are trimmed; an empty result becomes `project`. Names are mapped in sorted order and a
collision gets a numeric suffix (`a-b`, then `a-b-2`).

`paths[name]` in `projects.yaml` overrides the derived directory with an absolute path (`~` is expanded here).
Use it when one project must live somewhere the mapping cannot express, for example an existing checkout
outside the root.

## `projects.yaml`

```yaml
names:
  - desktop
  - core/cli
  - holiday-video
assignments:
  <conversation-id>: desktop
contexts:
  desktop: Always run task check before finishing.
groups:
  core/cli: core
types:
  holiday-video: content
paths:
  desktop: /Users/me/Repositories/desktop
selected: []
active: desktop
```

- `names` - every project. A project only referenced from `assignments` is added to the list on load.
- `assignments` - conversation id to project name. Ids come from `infer conversations list --all-projects --format json`
  (the `conversations` array, `id` and `title` fields).
- `contexts` - extra instructions appended to every prompt in that project.
- `groups` - sidebar group per project; it is also the relative parent directory under the root, so `..`,
  absolute paths and empty strings are ignored.
- `types` - only the literal `"content"`; a missing entry means a code project.
- `paths` - absolute per-project directory overrides.
- `selected` - internal to the "Init all projects" UI; leave it as you found it.
- `active` - the project open in the sidebar, restored on restart. Setting it to a name from `names` opens
  that project when your run finishes (the sidebar reloads the file then); remove it for no project.

The desktop rewrites the file with exactly these eight keys, so anything else you add is dropped. Read the
file first, change only the entries the task needs, keep everything else byte-for-byte, and write valid YAML.

## Opening a project in the sidebar

To open a project while the user watches (for example during a screen recording), drive the sidebar with
the `Computer` tool (target `app:Inference Gateway Desktop`):

- `press` matches the exact accessible label: `Select project <name>` opens a project, `Expand <name>` /
  `Collapse <name>` toggle a project's or group's chat list, `New chat` starts a chat. Never press
  `Delete project <name>`.
- If `press` finds nothing, read the tree once with the `accessibility` action and retry: the app's web
  content can be missing from the very first read.
- Fall back to a coordinate `click` only inside the window's bbox from that tree, and take a screenshot
  afterwards to confirm the project is open before telling the user it is.
- Opening a content project hides the sidebar on purpose, to make room for its timeline, and switching to a
  code project shows it again. That is not a failure, so don't reopen the sidebar to check your work.
  Confirm the switch from the composer's project chip (the project name next to `Leave project context`)
  or a screenshot. Press `Toggle sidebar` only if the user wants the sidebar back.

## Project types

- **code** (default): a git checkout. The desktop reads `AGENTS.md` (falling back to `CLAUDE.md`) from the
  project directory as project context and shows git status in the sidebar.
- **content**: a media folder. Switching a project to content in Settings > Projects installs `ffmpeg`,
  `whisper-cli`, the whisper model and the `video-editing` skill, and the desktop renders any
  `<stem>.timeline.json` in the directory as an editable timeline.

Setting `types[name]` to `"content"` in the file marks the project; the tool install runs the next time the
user opens Settings > Projects and toggles the type, so tell them that when you switch a project by hand.
Remove the entry to turn a project back into code.

## Sandbox

The agent's file tools can write to the current project directory, `/tmp`, and every project directory
resolved from `projects.yaml` (derived or overridden). Anything else prompts for approval. The grant is
computed when a session starts, so a moved project, a new override or a new `projects.root` applies to the
next session, not the current one. Say so when you finish.

Moving projects outside the root:

- a handful of projects: add `paths` overrides and leave `projects.root` alone;
- everything: change `projects.root` and move the directories, then drop overrides that now match the
  derived location.

## Recipes

**List projects and chats.** Read `projects.yaml`; run `infer conversations list --all-projects --format json`
for the chats. Chats whose id is not in `assignments` are unfiled.

**Rename a project.** Replace the name in `names`, `assignments`, `contexts`, `groups`, `types` and `paths`.
If the directory should follow, rename it too (`mv <old dir> <new dir>`) or add a `paths` override to the
old directory.

**Regroup or move a project.** Mirror what the desktop does: compute the old directory, compute the new one
(`<root>/<new group>/<leaf>`), refuse if the destination exists, `mkdir -p` the parent, `mv` the directory
(a plain rename, `git mv` is not needed), then set `groups[name]`. If the source directory does not exist,
just create the destination.

**Move all projects to a new root (e.g. `~/Repositories`).** Set `projects.root` in `config.yaml` to the
absolute path, `mkdir -p` it, and for every project without a `paths` override `mv` its directory from the
old derived location to the new one, keeping group subfolders. Leave overrides in place unless the user
asks to fold them in.

**Import existing repositories from a folder.** Walk the folder at most four levels deep, skipping dotdirs,
`node_modules` and `target`, and stop descending at the first directory containing `.git`. For each repo add
its directory name to `names`, its parent folder relative to the imported root (if any) to `groups`, and,
when the folder is not `projects.root`, its absolute path to `paths`.

**Assign chats to projects.** Match conversation titles to project names or ask the user; add
`assignments[id] = project`. Never delete an assignment without being told to.

## Guardrails

- Never edit the desktop's source code, and never write anywhere except `projects.yaml`, `config.yaml`, the
  projects root and directories named in `paths`.
- Never delete a project directory or its contents; moving is fine, removal is the user's job.
- Run `git status --porcelain` before moving a git checkout and ask before moving one with uncommitted
  changes or one that is not a git repository but is not empty.
- Show the planned edits (old path to new path, the diff) before applying them when more than one
  project is affected.
