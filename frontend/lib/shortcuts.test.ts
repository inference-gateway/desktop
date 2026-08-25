import { expect, test } from "bun:test";
import { matchShortcut, type KeyInput, type Shortcut } from "./shortcuts";

function key(overrides: Partial<KeyInput>): KeyInput {
  return {
    key: "",
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    repeat: false,
    defaultPrevented: false,
    inComposer: false,
    ...overrides,
  };
}

const cases: [string, Partial<KeyInput>, Shortcut | null][] = [
  ["cmd+n", { key: "n", metaKey: true }, "newChat"],
  ["ctrl+n", { key: "n", ctrlKey: true }, "newChat"],
  ["cmd+N with caps", { key: "N", metaKey: true }, "newChat"],
  ["cmd+shift+n passes through", { key: "n", metaKey: true, shiftKey: true }, null],
  ["cmd+alt+n passes through", { key: "n", metaKey: true, altKey: true }, null],
  ["plain n", { key: "n" }, null],
  ["escape", { key: "Escape" }, "cancel"],
  ["escape already handled by a popover", { key: "Escape", defaultPrevented: true }, null],
  ["escape with modifier", { key: "Escape", ctrlKey: true }, null],
  ["shift+tab in composer", { key: "Tab", shiftKey: true, inComposer: true }, "autoModeToggle"],
  ["shift+tab outside composer keeps focus nav", { key: "Tab", shiftKey: true }, null],
  ["plain tab in composer", { key: "Tab", inComposer: true }, null],
  ["held key repeat", { key: "Escape", repeat: true }, null],
];

for (const [name, input, expected] of cases) {
  test(name, () => {
    expect(matchShortcut(key(input))).toBe(expected);
  });
}
