import { expect, test } from "bun:test";
import { artifactKey, mediaType, unseenArtifacts } from "./artifacts";

const draft = { name: "draft-v1.mp4", size: 5_000_000, modified_at: "2026-11-12T10:00:00Z" };
const sfx = { name: "sfx-1-v1.mp3", size: 300_000, modified_at: "2026-11-12T09:00:00Z" };

test("a new artifact and a rewritten one are unseen, an unchanged one is not", () => {
  const seen = new Set([artifactKey(draft), artifactKey(sfx)]);
  const rewritten = { ...draft, size: 4_900_000, modified_at: "2026-11-12T10:05:00Z" };
  const added = { name: "draft-v2.mp4", size: 5_100_000, modified_at: "2026-11-12T10:10:00Z" };
  expect(unseenArtifacts(seen, [added, rewritten, sfx])).toEqual([added, rewritten]);
  expect(unseenArtifacts(seen, [draft, sfx])).toEqual([]);
});

test("audio, video and pictures have a media type, other files do not", () => {
  expect(mediaType("sfx-2-v3.MP3")).toBe("audio/mpeg");
  expect(mediaType("draft-v1.mp4")).toBe("video/mp4");
  expect(mediaType("demo.gif")).toBe("image/gif");
  expect(mediaType("edit.fcproj")).toBeUndefined();
  expect(mediaType("README")).toBeUndefined();
});
