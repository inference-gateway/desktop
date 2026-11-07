import { expect, test } from "bun:test";
import { filterFiles, insertMention, mentionAt, menuKey } from "./mentions";

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
