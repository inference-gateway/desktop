import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useRef, useState } from "react";
import { dropPoint, type Point } from "@/lib/file-drop";

const DROP_POSITION_IN_DEVICE_PIXELS = navigator.platform.toUpperCase().startsWith("WIN");

async function webviewFrameHeight(): Promise<number> {
  return (await getCurrentWebview().size()).height / window.devicePixelRatio;
}

function under(element: HTMLElement | null, position: Point, frameHeight: number): boolean {
  const { x, y } = dropPoint(position, {
    devicePixels: DROP_POSITION_IN_DEVICE_PIXELS,
    scale: window.devicePixelRatio,
    frameHeight,
    pageHeight: window.innerHeight,
  });
  const hit = document.elementFromPoint(x, y);
  return hit !== null && element?.contains(hit) === true;
}

/** Files dropped from the OS onto the element `ref` points at. Tauri hands the
 * page their paths, so the bytes never pass through the webview. `over` is true
 * while a file drag hovers the element. */
export function useFileDrop<E extends HTMLElement>(onDrop: (paths: string[]) => void) {
  const ref = useRef<E>(null);
  const [over, setOver] = useState(false);
  const onDropRef = useRef(onDrop);
  onDropRef.current = onDrop;

  useEffect(() => {
    let frameHeight = webviewFrameHeight();
    const unlisten = getCurrentWebview().onDragDropEvent(async ({ payload }) => {
      if (payload.type === "leave") {
        setOver(false);
        return;
      }
      if (payload.type === "enter") frameHeight = webviewFrameHeight();
      const hit = under(ref.current, payload.position, await frameHeight);
      if (payload.type !== "drop") {
        setOver(hit);
        return;
      }
      setOver(false);
      if (hit) onDropRef.current(payload.paths);
    });
    return () => {
      unlisten.then((stop) => stop());
    };
  }, []);

  return { ref, over };
}
