import { expect, test } from "bun:test";
import {
  configOf,
  draftOf,
  draftProblem,
  EMPTY_WORKSPACE,
  formatEnv,
  hasCapability,
  parseEnv,
  splitNames,
  volumesProblem,
  withCapability,
} from "./workspace-form";

const RECORDING = {
  id: "recording",
  label: "Recording",
  hint: "",
  env: [
    ["INFER_COMPUTER_USE_RECORDING_ENABLED", "true"],
    ["INFER_COMPUTER_USE_RECORDING_REQUIRE_APPROVAL", "false"],
  ] as [string, string][],
  user_data: "",
};
const BROWSER = {
  id: "browser-use",
  label: "Browser use",
  hint: "",
  env: [],
  user_data: "#!/bin/sh\ninstall_firefox\n",
};

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
    user_data: "#!/bin/sh\napt-get install -y firefox-esr\n",
    volumes: [{ name: "footage", claim: "raw-footage", mount_path: "/mnt/footage", read_only: true }],
  };
  expect(configOf(draftOf(config))).toEqual(config);
});

test("user data must be a script, the one cloud-init format a workspace runs", () => {
  const draft = { ...draftOf(EMPTY_WORKSPACE), name: "desktop", compute: "k3d" };
  expect(draftProblem({ ...draft, user_data: "#cloud-config\npackages:\n  - gimp\n" })).toContain("#!");
  expect(draftProblem({ ...draft, user_data: "#!/bin/sh\napt-get install -y gimp\n" })).toBeNull();
  expect(draftProblem({ ...draft, user_data: "" })).toBeNull();
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

test("a capability switched on and off again leaves what the person typed", () => {
  const typed = {
    ...draftOf(EMPTY_WORKSPACE),
    env: "INFER_AGENT_MODEL=openai/gpt-4o",
    user_data: "#!/bin/sh\napt-get install -y gimp\n",
  };
  const on = withCapability(withCapability(typed, RECORDING, true), BROWSER, true);
  expect(hasCapability(on, RECORDING)).toBe(true);
  expect(hasCapability(on, BROWSER)).toBe(true);
  expect(parseEnv(on.env).INFER_AGENT_MODEL).toBe("openai/gpt-4o");
  expect(on.user_data).toBe("#!/bin/sh\napt-get install -y gimp\n#!/bin/sh\ninstall_firefox\n");
  expect(withCapability(on, BROWSER, true)).toEqual(on);
  const off = withCapability(withCapability(on, BROWSER, false), RECORDING, false);
  expect(off).toEqual(typed);
});

test("a workspace lacks a capability once one of its variables is changed by hand, or it has no boot script", () => {
  const on = withCapability(draftOf(EMPTY_WORKSPACE), RECORDING, true);
  const edited = { ...on, env: on.env.replace("RECORDING_ENABLED=true", "RECORDING_ENABLED=false") };
  expect(hasCapability(edited, RECORDING)).toBe(false);
  expect(hasCapability(draftOf(EMPTY_WORKSPACE), RECORDING)).toBe(false);
  expect(hasCapability(draftOf(EMPTY_WORKSPACE), BROWSER)).toBe(false);
});

test("volumes need a name of their own, a claim and an absolute mount path", () => {
  const footage = { name: "footage", claim: "raw-footage", mount_path: "/mnt/footage", read_only: true };
  expect(volumesProblem([])).toBeNull();
  expect(volumesProblem([footage, { ...footage, name: "resolve", mount_path: "/opt/resolve" }])).toBeNull();
  expect(volumesProblem([{ ...footage, name: " " }])).toContain("volume 1 a name");
  expect(volumesProblem([footage, { ...footage, mount_path: "/mnt/other" }])).toContain("named footage");
  expect(volumesProblem([{ ...footage, claim: "" }])).toContain("claim");
  expect(volumesProblem([{ ...footage, mount_path: "mnt/footage" }])).toContain("starts with /");
});

test("a workspace with a volume it could not mount cannot be saved, and a saved one is trimmed", () => {
  const draft = { ...draftOf(EMPTY_WORKSPACE), name: "desktop", compute: "k3d" };
  const padded = { name: " footage ", claim: " raw-footage ", mount_path: " /mnt/footage ", read_only: false };
  expect(draftProblem({ ...draft, volumes: [{ ...padded, claim: "" }] })).toContain("claim");
  expect(draftProblem({ ...draft, volumes: [padded] })).toBeNull();
  expect(configOf({ ...draft, volumes: [padded] }).volumes).toEqual([
    { name: "footage", claim: "raw-footage", mount_path: "/mnt/footage", read_only: false },
  ]);
});

test("a read-only volume on a local cluster folder is saved on the folder's read-only claim", () => {
  const draft = { ...draftOf(EMPTY_WORKSPACE), name: "desktop", compute: "infer-k3s" };
  const footage = { name: "footage", claim: "folder-footage", mount_path: "/mnt/raw", read_only: true };
  expect(configOf({ ...draft, volumes: [footage] }).volumes[0].claim).toBe("folder-footage-ro");
});
