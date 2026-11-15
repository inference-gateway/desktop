import { expect, test } from "bun:test";
import { downloadArguments, pickedDownload } from "./hub-models";

const FILES = [
  { path: "config.json", size: 748 },
  { path: "unet.safetensors", size: 1699963904 },
  { path: "vae.safetensors", size: 167335342 },
];

test("a download is the picked files and their total size", () => {
  const picked = new Set(["config.json", "vae.safetensors", "gone.bin"]);
  expect(pickedDownload(FILES, picked)).toEqual({ paths: ["config.json", "vae.safetensors"], bytes: 748 + 167335342 });
  expect(pickedDownload(FILES, new Set())).toEqual({ paths: [], bytes: 0 });
});

test("picking every file asks the CLI for the whole repository", () => {
  expect(downloadArguments(FILES, new Set(FILES.map((f) => f.path)))).toEqual([]);
  expect(downloadArguments(FILES, new Set(["config.json"]))).toEqual(["config.json"]);
});
