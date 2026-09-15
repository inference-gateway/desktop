import { expect, test } from "bun:test";
import { matchApprovalShortcut, matchShortcut, type ApprovalShortcut, type KeyInput, type Shortcut } from "./shortcuts";

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
  ["escape still cancels when a popover preventDefaulted it", { key: "Escape", defaultPrevented: true }, "cancel"],
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

const approvalCases: [string, KeyInput, ApprovalShortcut | null][] = [
  ["a in the composer approves", key({ key: "a", inComposer: true, editable: true }), "approve"],
  ["d in the composer denies", key({ key: "d", inComposer: true, editable: true }), "deny"],
  ["a with caps approves", key({ key: "A" }), "approve"],
  ["a outside any editable approves", key({ key: "a" }), "approve"],
  ["other letters keep typing in the composer", key({ key: "x", inComposer: true, editable: true }), null],
  ["a typed into another editable passes through", key({ key: "a", editable: true }), null],
  ["modifier combos pass through", key({ key: "a", metaKey: true }), null],
  ["held key repeat", key({ key: "a", repeat: true }), null],
  ["preventDefaulted key passes through", key({ key: "a", defaultPrevented: true }), null],
];

for (const [name, input, expected] of approvalCases) {
  test(name, () => {
    expect(matchApprovalShortcut(input)).toBe(expected);
  });
}
