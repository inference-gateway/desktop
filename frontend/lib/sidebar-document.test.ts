import { expect, test } from "bun:test";
import { parseSidebarDocument, serializeSidebarDocument } from "./sidebar-document";

test("a stored document reads back as written", () => {
  const raw = JSON.stringify({
    assignments: { c1: "api" },
    names: ["api", "site"],
    contexts: { api: "ctx" },
    paths: { api: "/src/api" },
    groups: { api: "work" },
    types: { site: "content" },
    agents: { c1: "helper" },
    workspaces: { c1: "" },
    titles: { c1: "First chat" },
    selected: ["site"],
    active: "api",
  });
  const doc = parseSidebarDocument(raw);
  expect(doc.names).toEqual(["api", "site"]);
  expect(doc.selected).toEqual(new Set(["site"]));
  expect(doc.workspaces).toEqual({ c1: "" });
  expect(JSON.parse(serializeSidebarDocument(doc))).toEqual(JSON.parse(raw));
});

test("an empty or malformed document reads as empty, and no active project is not stored", () => {
  const doc = parseSidebarDocument(JSON.stringify({ names: "nope", assignments: null, types: { a: "code" } }));
  expect(doc.names).toEqual([]);
  expect(doc.types).toEqual({});
  expect(doc.active).toBeNull();
  expect("active" in JSON.parse(serializeSidebarDocument(doc))).toBe(false);
});

test("a chat's project becomes a project, and blank entries are dropped", () => {
  const doc = parseSidebarDocument(
    JSON.stringify({
      names: ["a", " ", 3],
      assignments: { c1: "b", c2: " " },
      contexts: { a: " " },
      agents: { c1: "" },
    }),
  );
  expect(doc.names).toEqual(["a", "b"]);
  expect(doc.assignments).toEqual({ c1: "b" });
  expect(doc.contexts).toEqual({});
  expect(doc.agents).toEqual({});
});

test("a selection or active project that no longer exists is dropped", () => {
  const doc = parseSidebarDocument(JSON.stringify({ names: ["a"], selected: ["a", "gone"], active: "gone" }));
  expect(doc.selected).toEqual(new Set(["a"]));
  expect(doc.active).toBeNull();
});
