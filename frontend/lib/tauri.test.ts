import { expect, test } from "bun:test";
import { CommandError, commandError } from "./tauri";

test("a backend error with a kind becomes a CommandError that prints as its message", () => {
  const error = commandError({ kind: "unsupported_input", message: 'unsupported screen input "click"' });
  expect(error).toBeInstanceOf(CommandError);
  expect((error as CommandError).kind).toBe("unsupported_input");
  expect(String(error)).toBe('unsupported screen input "click"');
  expect(`Failed to take over: ${error}`).toBe('Failed to take over: unsupported screen input "click"');
});

test("a plain string error passes through unchanged", () => {
  expect(commandError("no such chat")).toBe("no such chat");
});
