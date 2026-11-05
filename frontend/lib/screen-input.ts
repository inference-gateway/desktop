/** One input of the person on a screen they took over, in the frame space the daemon serves the screen in. */
export type ScreenInput =
  | { kind: "click" | "double_click"; x: number; y: number; button: "left" | "right" | "middle" }
  | { kind: "scroll"; direction: "vertical" | "horizontal"; amount: number }
  | { kind: "type"; text: string }
  | { kind: "key"; combo: string };

type Size = { width: number; height: number };
type Key = Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">;

const SPECIAL_KEYS: Record<string, string> = {
  Escape: "escape",
  Backspace: "backspace",
  Enter: "enter",
  Tab: "tab",
  Delete: "delete",
  Home: "home",
  End: "end",
  PageUp: "pageup",
  PageDown: "pagedown",
  ArrowUp: "up",
  ArrowDown: "down",
  ArrowLeft: "left",
  ArrowRight: "right",
};

const MAX_WHEEL_CLICKS = 10;
const WHEEL_DELTA_PER_CLICK = 100;

/** Maps a pointer position inside the displayed image onto the frame, or null while either has no size. */
export function framePoint(
  pointer: { x: number; y: number },
  image: Size & { left: number; top: number },
  frame: Size,
): { x: number; y: number } | null {
  if (image.width <= 0 || image.height <= 0 || frame.width <= 0 || frame.height <= 0) return null;
  const scale = (offset: number, shown: number, real: number) =>
    Math.min(Math.max(Math.round((offset * real) / shown), 0), real);
  return {
    x: scale(pointer.x - image.left, image.width, frame.width),
    y: scale(pointer.y - image.top, image.height, frame.height),
  };
}

export function mouseButton(button: number): "left" | "right" | "middle" {
  return button === 2 ? "right" : button === 1 ? "middle" : "left";
}

function keyName(key: string): string | null {
  if (SPECIAL_KEYS[key]) return SPECIAL_KEYS[key];
  return /^F\d{1,2}$/.test(key) ? key.toLowerCase() : null;
}

/** The input a key press sends: plain text types, special keys and modifier chords tap as a combo. */
export function keyInput(event: Key): ScreenInput | null {
  const chord = event.ctrlKey || event.metaKey || event.altKey;
  const printable = event.key.length === 1;
  if (!chord) {
    const name = keyName(event.key);
    if (name) return { kind: "key", combo: name };
    return printable ? { kind: "type", text: event.key } : null;
  }
  const key = keyName(event.key) ?? (event.key === " " ? "space" : printable ? event.key.toLowerCase() : null);
  if (!key) return null;
  const modifiers = [
    event.ctrlKey && "ctrl",
    event.metaKey && "cmd",
    event.altKey && "alt",
    event.shiftKey && "shift",
  ].filter(Boolean);
  return { kind: "key", combo: [...modifiers, key].join("+") };
}

/** The scroll inputs of one wheel event, a positive amount wheeling down or right. */
export function wheelInputs(deltaX: number, deltaY: number): ScreenInput[] {
  const clicks = (delta: number) =>
    Math.sign(delta) * Math.min(Math.max(Math.round(Math.abs(delta) / WHEEL_DELTA_PER_CLICK), 1), MAX_WHEEL_CLICKS);
  const scroll = (direction: "vertical" | "horizontal", delta: number): ScreenInput[] =>
    delta ? [{ kind: "scroll", direction, amount: clicks(delta) }] : [];
  return [...scroll("vertical", deltaY), ...scroll("horizontal", deltaX)];
}
