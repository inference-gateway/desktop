import { expect, test } from "bun:test";
import { selectableVersions, stripV, versionAction } from "./updates";

test("a release tag loses its v prefix, nothing else", () => {
  expect(stripV("v0.63.1")).toBe("0.63.1");
  expect(stripV("  v1.2.3  ")).toBe("1.2.3");
  expect(stripV("v")).toBe("v");
});

test("a row offers the newest 5 tags and nothing on dev builds or unknown releases", () => {
  expect(
    selectableVersions({
      name: "CLI",
      current: "0.63.0",
      latest: "0.63.1",
      outdated: true,
      versions: ["v0.63.1", "v0.63.0", "v0.62.0", "v0.61.0", "v0.60.0", "v0.59.0"],
    }),
  ).toEqual(["v0.63.1", "v0.63.0", "v0.62.0", "v0.61.0", "v0.60.0"]);
  expect(
    selectableVersions({ name: "Desktop", current: "dev", latest: "", outdated: false, versions: ["v0.63.1"] }),
  ).toEqual([]);
  expect(selectableVersions({ name: "CLI", current: "0.63.0", latest: "", outdated: false })).toEqual([]);
});

test("an older install means an upgrade and a newer one a downgrade, numerically", () => {
  expect(versionAction("0.158.0", "v0.158.1")).toBe("Upgrade");
  expect(versionAction("0.159.0", "v0.158.0")).toBe("Downgrade");
  expect(versionAction("0.9.0", "v0.10.0")).toBe("Upgrade");
  expect(versionAction("0.158.0", "v0.158.0")).toBeNull();
  expect(versionAction("0.158.0", "0.158.0.1")).toBe("Upgrade");
  expect(versionAction("dev", "0.158.0")).toBeNull();
  expect(versionAction("0.158.0", "not-a-version")).toBeNull();
});
