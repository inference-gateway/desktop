import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { FLUENT_EMOJIS, fluentAnimatedUrl, fluentStaticUrl } from "./fluent-emoji";

const config = JSON.parse(readFileSync(new URL("../../backend/tauri.conf.json", import.meta.url), "utf8"));
const imageSources: string[] = config.app.security.csp["img-src"].split(" ");

test("the content security policy lets every emoji picture load", () => {
  const allowed = (url: string) => imageSources.some((source) => source.endsWith("/") && url.startsWith(source));
  for (const emoji of FLUENT_EMOJIS) {
    expect(allowed(fluentStaticUrl(emoji))).toBe(true);
    expect(allowed(fluentAnimatedUrl(emoji))).toBe(true);
  }
});

test("the content security policy loads images from no other remote host", () => {
  expect(imageSources).not.toContain("https:");
  expect(imageSources).not.toContain("http:");
  expect(imageSources).not.toContain("*");
});
