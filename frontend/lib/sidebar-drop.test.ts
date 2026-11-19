import { describe, expect, test } from "bun:test";
import { groupTarget, projectTarget, sidebarDrop, UNGROUPED_TARGET } from "./sidebar-drop";

describe("sidebarDrop", () => {
  const chats = { kind: "chats" as const, ids: ["a", "b"] };
  const project = { kind: "project" as const, name: "web" };

  test("chats go into the project they are dropped on, even one with a colon in its name", () => {
    expect(sidebarDrop(chats, projectTarget("api:v2"))).toEqual({ kind: "assign", ids: ["a", "b"], project: "api:v2" });
  });

  test("chats dropped on the ungrouped list leave their project", () => {
    expect(sidebarDrop(chats, UNGROUPED_TARGET)).toEqual({ kind: "unassign", ids: ["a", "b"] });
  });

  test("a project goes into the group it is dropped on", () => {
    expect(sidebarDrop(project, groupTarget("Work"))).toEqual({ kind: "moveProject", name: "web", group: "Work" });
  });

  test("a target that does not take the drag does nothing", () => {
    expect(sidebarDrop(chats, groupTarget("Work"))).toBeNull();
    expect(sidebarDrop(project, projectTarget("api"))).toBeNull();
    expect(sidebarDrop(project, UNGROUPED_TARGET)).toBeNull();
  });
});
