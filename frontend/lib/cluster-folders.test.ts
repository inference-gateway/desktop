import { expect, test } from "bun:test";
import { folderClaim, folderOfClaim, foldersProblem, onFolder, onFolderClaim } from "./cluster-folders";

test("a folder has a writable claim and a read-only one, and both name the folder", () => {
  expect(folderClaim("footage", false)).toBe("folder-footage");
  expect(folderClaim("footage", true)).toBe("folder-footage-ro");
  expect(folderOfClaim("folder-footage")).toBe("footage");
  expect(folderOfClaim("folder-footage-ro")).toBe("footage");
  expect(folderOfClaim("davinci-resolve")).toBeNull();
});

test("a read-only volume on a folder mounts its read-only claim, and any other claim stays as written", () => {
  const volume = { name: "footage", claim: "folder-footage", mount_path: "/mnt/raw", read_only: true };
  expect(onFolderClaim(volume).claim).toBe("folder-footage-ro");
  expect(onFolderClaim({ ...volume, claim: "folder-footage-ro", read_only: false }).claim).toBe("folder-footage");
  expect(onFolderClaim({ ...volume, claim: "davinci-resolve" }).claim).toBe("davinci-resolve");
});

test("folders need a claim-safe name of their own and an absolute path", () => {
  const footage = { name: "footage", path: "/Users/me/Movies/raw", read_only: true };
  expect(foldersProblem([])).toBeNull();
  expect(foldersProblem([footage, { ...footage, name: "apps" }])).toBeNull();
  expect(foldersProblem([{ ...footage, name: "" }])).toContain("folder 1 a name");
  expect(foldersProblem([{ ...footage, name: "Raw_Footage" }])).toContain("lowercase");
  expect(foldersProblem([{ ...footage, name: "footage-ro" }])).toContain("-ro");
  expect(foldersProblem([footage, footage])).toContain("named footage");
  expect(foldersProblem([{ ...footage, path: "Movies/raw" }])).toContain("starts with /");
});

test("a volume put on a folder takes its claim and its name, and a read-only folder makes it read-only", () => {
  const empty = { name: "", claim: "", mount_path: "/mnt/raw", read_only: false };
  expect(onFolder(empty, { name: "footage", path: "/Users/me/raw", read_only: false })).toEqual({
    name: "footage",
    claim: "folder-footage",
    mount_path: "/mnt/raw",
    read_only: false,
  });
  const locked = onFolder({ ...empty, name: "clips" }, { name: "footage", path: "/Users/me/raw", read_only: true });
  expect(locked).toEqual({ name: "clips", claim: "folder-footage-ro", mount_path: "/mnt/raw", read_only: true });
});
