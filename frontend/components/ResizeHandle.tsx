import { useRef, type PointerEvent as ReactPointerEvent } from "react";
import { cn } from "@/lib/utils";

// The drag strip on one edge of a side panel. Dragging away from the panel
// widens it; double-click resets. The panel owns the width state.
export function ResizeHandle({
  edge,
  width,
  clamp,
  onChange,
  onEnd,
  onReset,
  onDragging,
  title,
}: {
  edge: "left" | "right";
  width: number;
  clamp: (w: number) => number;
  onChange: (w: number) => void;
  onEnd: () => void;
  onReset: () => void;
  onDragging?: (dragging: boolean) => void;
  title: string;
}) {
  const drag = useRef<{ x: number; w: number } | null>(null);
  const sign = edge === "right" ? 1 : -1;
  const end = () => {
    if (!drag.current) return;
    drag.current = null;
    onDragging?.(false);
    onEnd();
  };
  return (
    <div
      aria-hidden="true"
      title={title}
      className={cn(
        "absolute inset-y-0 z-10 w-1.5 cursor-col-resize touch-none hover:bg-primary/30",
        edge === "right" ? "right-0" : "left-0",
      )}
      onPointerDown={(e: ReactPointerEvent<HTMLDivElement>) => {
        drag.current = { x: e.clientX, w: width };
        onDragging?.(true);
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        if (drag.current) onChange(clamp(drag.current.w + sign * (e.clientX - drag.current.x)));
      }}
      onPointerUp={end}
      onPointerCancel={end}
      onDoubleClick={onReset}
    />
  );
}
