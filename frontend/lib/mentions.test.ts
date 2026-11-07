import { expect, test } from "bun:test";
import { filterFiles, filterIssues, insertMention, mentionAt, menuRows, menuKey } from "./mentions";

const ISSUES = [
  { number: 57, title: "Fix the flaky test", state: "OPEN" },
  { number: 42, title: "Add a dark mode toggle", state: "OPEN" },
];

test("mentionAt finds the @ token that ends at the cursor", () => {
  expect(mentionAt("@", 1)).toEqual({ trigger: "@", start: 0, query: "" });
  expect(mentionAt("read @src/ma", 12)).toEqual({ trigger: "@", start: 5, query: "src/ma" });
  expect(mentionAt("line one\n@READ", 14)).toEqual({ trigger: "@", start: 9, query: "READ" });
  expect(mentionAt("@README.md and more", 3)).toEqual({ trigger: "@", start: 0, query: "RE" });
});

test("mentionAt ignores an @ inside a word or a token the cursor left", () => {
  expect(mentionAt("mail me@example.com", 19)).toBeNull();
  expect(mentionAt("@README.md ", 11)).toBeNull();
  expect(mentionAt("no trigger", 10)).toBeNull();
});

test("mentionAt finds a # token but not one inside a word or a URL", () => {
  expect(mentionAt("#", 1)).toEqual({ trigger: "#", start: 0, query: "" });
  expect(mentionAt("fix #57", 7)).toEqual({ trigger: "#", start: 4, query: "57" });
  expect(mentionAt("written in C#", 13)).toBeNull();
  expect(mentionAt("see https://example.com/page#intro", 34)).toBeNull();
});

test("filterIssues matches the number or the title, ignoring case", () => {
  expect(filterIssues(ISSUES, "")).toEqual(ISSUES);
  expect(filterIssues(ISSUES, "57")).toEqual([ISSUES[0]]);
  expect(filterIssues(ISSUES, "Dark")).toEqual([ISSUES[1]]);
  expect(filterIssues(ISSUES, "99")).toEqual([]);
  expect(filterIssues([], "")).toEqual([]);
});

test("menuRows offers issues for # and files for @", () => {
  expect(menuRows({ trigger: "#", start: 0, query: "flaky" }, ["README.md"], ISSUES)).toEqual([
    { value: "#57", label: "#57 Fix the flaky test" },
  ]);
  expect(menuRows({ trigger: "@", start: 0, query: "read" }, ["README.md"], ISSUES)).toEqual([
    { value: "@README.md", label: "README.md" },
  ]);
  expect(menuRows({ trigger: "#", start: 0, query: "" }, ["README.md"], [])).toEqual([]);
  expect(menuRows(null, ["README.md"], ISSUES)).toEqual([]);
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

test("insertMention replaces the token and keeps the text around it", () => {
  const text = "explain @RE please";
  const mention = mentionAt(text, 11)!;
  expect(insertMention(text, 11, mention, "@README.md")).toEqual({
    text: "explain @README.md please",
    cursor: 18,
  });
  expect(insertMention("@", 1, mentionAt("@", 1)!, "@README.md")).toEqual({ text: "@README.md ", cursor: 11 });
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
