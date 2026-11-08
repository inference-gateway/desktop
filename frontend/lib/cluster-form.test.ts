import { expect, test } from "bun:test";
import { clusterConfigOf, clusterDraftOf, clusterProblem, EMPTY_CLUSTER } from "./cluster-form";

test("a cluster draft turns back into the config it came from", () => {
  const config = { name: "k3d", context: "k3d-agents", namespace: "agents", gateway_env: ["ANTHROPIC_API_KEY"] };
  expect(clusterConfigOf(clusterDraftOf(config))).toEqual(config);
});

test("a cluster needs a label-safe name and a namespace", () => {
  const draft = clusterDraftOf(EMPTY_CLUSTER);
  expect(clusterProblem(draft)).toContain("name");
  expect(clusterProblem({ ...draft, name: "has space" })).toContain("letters, digits");
  expect(clusterProblem({ ...draft, name: "-leading" })).toContain("letters, digits");
  expect(clusterProblem({ ...draft, name: "a".repeat(64) })).toContain("letters, digits");
  expect(clusterProblem({ ...draft, name: "k3d", namespace: " " })).toContain("namespace");
  expect(clusterProblem({ ...draft, name: "Prod.EU_1" })).toBeNull();
});
