import { describe, expect, test } from "bun:test";
import { dropPoint } from "./file-drop";

describe("dropPoint", () => {
  test("takes the title bar the webview frame runs under off a macOS drop", () => {
    const view = { devicePixels: false, scale: 2, frameHeight: 760, pageHeight: 728 };
    expect(dropPoint({ x: 341, y: 740 }, view)).toEqual({ x: 341, y: 708 });
  });

  test("scales a Windows drop from device pixels", () => {
    const view = { devicePixels: true, scale: 1.5, frameHeight: 728, pageHeight: 728 };
    expect(dropPoint({ x: 511.5, y: 1062 }, view)).toEqual({ x: 341, y: 708 });
  });

  test("keeps a drop where the frame is the page", () => {
    const view = { devicePixels: false, scale: 1, frameHeight: 728, pageHeight: 728 };
    expect(dropPoint({ x: 341, y: 708 }, view)).toEqual({ x: 341, y: 708 });
  });
});
