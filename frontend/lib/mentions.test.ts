import { expect, test } from "bun:test";
import {
  filterAgents,
  filterFiles,
  filterIssues,
  insertMention,
  mentionAt,
  menuKey,
  menuRows,
  type AgentFace,
} from "./mentions";

const ISSUES = [
  { number: 57, title: "Fix the flaky test", state: "OPEN" },
  { number: 42, title: "Add a dark mode toggle", state: "OPEN" },
];
const REVIEWER: AgentFace = { name: "reviewer", emoji: "", avatar: null };
const AGENTS = [REVIEWER];

test("mentionAt finds the token that starts with a table trigger", () => {
  const triggers = ["@", "#", "^", "§"];
  expect(mentionAt("@", 1, triggers)).toEqual({ trigger: "@", start: 0, query: "" });
  expect(mentionAt("read @src/ma", 12, triggers)).toEqual({ trigger: "@", start: 5, query: "src/ma" });
  expect(mentionAt("line one\n@READ", 14, triggers)).toEqual({ trigger: "@", start: 9, query: "READ" });
  expect(mentionAt("@README.md and more", 3, triggers)).toEqual({ trigger: "@", start: 0, query: "RE" });
  expect(mentionAt("work on ^1", 10, triggers)).toEqual({ trigger: "^", start: 8, query: "1" });
  expect(mentionAt("see §", 5, triggers)).toEqual({ trigger: "§", start: 4, query: "" });
});

test("mentionAt ignores a trigger inside a word or after a token the cursor left", () => {
  const triggers = ["@", "#"];
  expect(mentionAt("mail me@example.com", 19, triggers)).toBeNull();
  expect(mentionAt("@README.md ", 11, triggers)).toBeNull();
  expect(mentionAt("no trigger", 10, triggers)).toBeNull();
  expect(mentionAt("written in C#", 13, triggers)).toBeNull();
  expect(mentionAt("see https://example.com/page#intro", 34, triggers)).toBeNull();
});

test("a trigger that recurs in the token opens nothing", () => {
  const triggers = ["#"];
  expect(mentionAt("two #5#7", 8, triggers)).toBeNull();
  expect(mentionAt("two #57", 7, triggers)).toEqual({ trigger: "#", start: 4, query: "57" });
});

test("filterIssues matches the number or the title, ignoring case", () => {
  expect(filterIssues(ISSUES, "")).toEqual(ISSUES);
  expect(filterIssues(ISSUES, "57")).toEqual([ISSUES[0]]);
  expect(filterIssues(ISSUES, "Dark")).toEqual([ISSUES[1]]);
  expect(filterIssues(ISSUES, "99")).toEqual([]);
  expect(filterIssues([], "")).toEqual([]);
});

test("menuRows offers the trigger's sections in its table order", () => {
  const table = { "@": ["files", "agents"], "#": ["issues"], "^": ["issues"] };
  const sources = { files: ["README.md"], issues: ISSUES, agents: AGENTS };
  expect(menuRows({ trigger: "#", start: 0, query: "flaky" }, table, sources)).toEqual([
    { value: "#57", label: "#57 Fix the flaky test" },
  ]);
  expect(menuRows({ trigger: "@", start: 0, query: "" }, table, sources)).toEqual([
    { value: "@README.md", label: "README.md" },
    { value: "@reviewer", label: "reviewer", agent: REVIEWER },
  ]);
  expect(menuRows({ trigger: "^", start: 0, query: "" }, table, sources)).toEqual([
    { value: "#57", label: "#57 Fix the flaky test" },
    { value: "#42", label: "#42 Add a dark mode toggle" },
  ]);
  expect(menuRows(null, table, sources)).toEqual([]);
});

test("the table is used as is, so a trigger it leaves out opens nothing", () => {
  const sources = { files: ["README.md"], issues: ISSUES, agents: AGENTS };
  expect(menuRows({ trigger: "@", start: 0, query: "" }, { "^": ["issues"] }, sources)).toEqual([]);
  expect(mentionAt("@", 1, [])).toBeNull();
});

test("filterAgents keeps the names containing the query, ignoring case", () => {
  expect(filterAgents(AGENTS, "")).toEqual(AGENTS);
  expect(filterAgents(AGENTS, "view")).toEqual([REVIEWER]);
  expect(filterAgents(AGENTS, "reviewer")).toEqual([REVIEWER]);
  expect(filterAgents(AGENTS, "nothing")).toEqual([]);
});

test("filterFiles keeps the paths containing the query, ignoring case", () => {
  const files = ["README.md", "backend/src/main.rs", "frontend/App.tsx", "~/.infer/config.yaml"];
  expect(filterFiles(files, "")).toEqual(files);
  expect(filterFiles(files, "read")).toEqual(["README.md"]);
  expect(filterFiles(files, "SRC/")).toEqual(["backend/src/main.rs"]);
  expect(filterFiles(files, "nothing")).toEqual([]);
  expect(
    filterFiles(
      Array.from({ length: 80 }, (_, i) => `f${i}`),
      "f",
    ),
  ).toHaveLength(50);
});

test("the whole menu caps at the first 50 rows of its sections", () => {
  const rows = menuRows(
    { trigger: "@", start: 0, query: "" },
    { "@": ["files", "agents"] },
    { files: Array.from({ length: 60 }, (_, i) => `f${i}`), issues: [], agents: AGENTS },
  );
  expect(rows).toHaveLength(50);
});

test("insertMention replaces the token and keeps the text around it", () => {
  const text = "explain @RE please";
  const mention = mentionAt(text, 11, ["@"])!;
  expect(insertMention(text, 11, mention, "@README.md")).toEqual({
    text: "explain @README.md please",
    cursor: 18,
  });
  expect(insertMention("@", 1, mentionAt("@", 1, ["@"])!, "@README.md")).toEqual({
    text: "@README.md ",
    cursor: 11,
  });
});

test("menuKey moves within the rows, picks on Enter and closes on Escape", () => {
  expect(menuKey("ArrowDown", 0, 3)).toBe(1);
  expect(menuKey("ArrowDown", 2, 3)).toBe(2);
  expect(menuKey("ArrowUp", 1, 3)).toBe(0);
  expect(menuKey("ArrowUp", 0, 3)).toBe(0);
  expect(menuKey("Enter", 1, 3)).toBe("pick");
  expect(menuKey("Escape", 1, 3)).toBe("close");
  expect(menuKey("a", 1, 3)).toBeNull();
});
