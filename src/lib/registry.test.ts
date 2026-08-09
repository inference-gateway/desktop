import { expect, test } from "bun:test";
import { toCatalogAgents } from "./registry";

test("toCatalogAgents maps name/description/version/skills", () => {
  const out = toCatalogAgents({
    agents: [
      {
        metadata: { name: "documentation-agent", description: "docs agent", version: "0.3.4" },
        spec: { skills: [{ name: "lookup" }, { name: "search" }] },
      },
    ],
  });
  expect(out).toEqual([
    { name: "documentation-agent", description: "docs agent", version: "0.3.4", skills: ["lookup", "search"] },
  ]);
});

test("toCatalogAgents falls back on missing fields", () => {
  expect(toCatalogAgents({ agents: [{}] })).toEqual([{ name: "", description: "", version: "", skills: [] }]);
});

test("toCatalogAgents returns [] for empty/malformed input", () => {
  expect(toCatalogAgents(undefined)).toEqual([]);
  expect(toCatalogAgents(null)).toEqual([]);
  expect(toCatalogAgents({})).toEqual([]);
});

test("toCatalogAgents drops nameless skills", () => {
  const out = toCatalogAgents({ agents: [{ spec: { skills: [{ name: "a" }, {}, { name: "" }] } }] });
  expect(out[0].skills).toEqual(["a"]);
});
