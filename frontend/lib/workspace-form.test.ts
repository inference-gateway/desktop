import { expect, test } from "bun:test";
import { configOf, draftOf, draftProblem, EMPTY_WORKSPACE, formatEnv, parseEnv, splitNames } from "./workspace-form";

test("names split on commas, spaces and new lines", () => {
  expect(splitNames("OPENAI_API_KEY, GH_TOKEN\nNPM_TOKEN ")).toEqual(["OPENAI_API_KEY", "GH_TOKEN", "NPM_TOKEN"]);
  expect(splitNames("  ")).toEqual([]);
});

test("an environment keeps values with equals signs and drops lines without a name", () => {
  const env = parseEnv("INFER_AGENT_MODEL=openai/gpt-4o\n\nFLAGS=a=b\n=orphan\nnoise");
  expect(env).toEqual({ INFER_AGENT_MODEL: "openai/gpt-4o", FLAGS: "a=b" });
  expect(parseEnv(formatEnv(env))).toEqual(env);
});

test("a draft turns back into the config it came from", () => {
  const config = {
    ...EMPTY_WORKSPACE,
    name: "dev",
    context: "k3d-dev",
    namespace: "agents",
    gateway_env: ["OPENAI_API_KEY"],
    forward_env: ["GH_TOKEN", "NPM_TOKEN"],
    env: { INFER_AGENT_MODEL: "openai/gpt-4o" },
  };
  expect(configOf(draftOf(config))).toEqual(config);
});

test("a kubernetes workspace needs a name, a context and a namespace", () => {
  const draft = draftOf(EMPTY_WORKSPACE);
  expect(draftProblem(draft)).toContain("name");
  expect(draftProblem({ ...draft, name: "dev" })).toContain("context");
  expect(draftProblem({ ...draft, name: "dev", context: "k3d-dev" })).toContain("namespace");
  expect(draftProblem({ ...draft, name: "dev", context: "k3d-dev", namespace: "agents" })).toBeNull();
});

test("a workspace on a cluster needs only a name, since the cluster gives it its connection", () => {
  const draft = { ...draftOf(EMPTY_WORKSPACE), name: "reviewer", compute: "k3d" };
  expect(draftProblem(draft)).toBeNull();
  expect(configOf(draft).compute).toBe("k3d");
});
