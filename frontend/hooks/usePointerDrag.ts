import { useCallback, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";

const DRAG_THRESHOLD_PX = 4;
const DROP_TARGET_ATTRIBUTE = "data-drop-target";

function dropTargetAt(x: number, y: number): string | null {
  return (
    document.elementFromPoint(x, y)?.closest(`[${DROP_TARGET_ATTRIBUTE}]`)?.getAttribute(DROP_TARGET_ATTRIBUTE) ?? null
  );
}

/** Drops the click a drag ends with, so the row it started on does not open. */
function swallowNextClick() {
  const stop = (e: MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
  };
  window.addEventListener("click", stop, { capture: true, once: true });
  setTimeout(() => window.removeEventListener("click", stop, { capture: true }), 0);
}

/** In-page drag-and-drop on pointer events, which keeps working once the window
 * takes native file drops - HTML5 drag-and-drop stops reaching the page then.
 * A source calls `start` on pointerdown, a target carries `data-drop-target`,
 * and `over` is the target under the pointer while a drag is on. The pointer is
 * captured once a press turns into a drag, so a release outside the window ends it. */
export function usePointerDrag<P>(onDrop: (payload: P, target: string, point: { x: number; y: number }) => void) {
  const [over, setOver] = useState<string | null>(null);
  const onDropRef = useRef(onDrop);
  onDropRef.current = onDrop;

  const start = useCallback((e: ReactPointerEvent, payload: P) => {
    if (e.button !== 0) return;
    const origin = { x: e.clientX, y: e.clientY };
    const source = e.currentTarget;
    const pointerId = e.pointerId;
    let dragging = false;

    const move = (ev: PointerEvent) => {
      if (!dragging && Math.hypot(ev.clientX - origin.x, ev.clientY - origin.y) < DRAG_THRESHOLD_PX) return;
      if (!dragging) source.setPointerCapture(pointerId);
      dragging = true;
      setOver(dropTargetAt(ev.clientX, ev.clientY));
    };
    const finish = (ev: PointerEvent, drop: boolean) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      if (!dragging) return;
      setOver(null);
      swallowNextClick();
      const target = drop ? dropTargetAt(ev.clientX, ev.clientY) : null;
      if (target !== null) onDropRef.current(payload, target, { x: ev.clientX, y: ev.clientY });
    };
    const up = (ev: PointerEvent) => finish(ev, true);
    const cancel = (ev: PointerEvent) => finish(ev, false);

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
  }, []);

  return { over, start };
}
