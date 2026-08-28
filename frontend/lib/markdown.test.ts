import { describe, expect, test } from "bun:test";
import { handleLinkClick } from "./markdown";

const click = (href: string) => {
  let prevented = false;
  let opened: string | undefined;
  const event = {
    target: { closest: () => ({ getAttribute: () => href }) },
    preventDefault: () => {
      prevented = true;
    },
  };
  handleLinkClick(event, (url) => {
    opened = url;
  });
  return { prevented, opened };
};

describe("handleLinkClick", () => {
  test("routes http(s) links to the browser, blocking webview navigation", () => {
    expect(click("https://example.com/docs")).toEqual({ prevented: true, opened: "https://example.com/docs" });
    expect(click("http://localhost:8080/x?y=1")).toEqual({ prevented: true, opened: "http://localhost:8080/x?y=1" });
  });

  test("leaves non-http targets (mailto, anchors, missing) to default behavior", () => {
    expect(click("mailto:a@b.c")).toEqual({ prevented: false, opened: undefined });
    expect(click("#anchor")).toEqual({ prevented: false, opened: undefined });
    expect(click("")).toEqual({ prevented: false, opened: undefined });
  });
});
