import { cn } from "@/lib/utils";
import { useDesktop } from "@/store";

export function StatusBar() {
  const { statusText, statusError } = useDesktop();
  return (
    <div
      id="status-bar"
      role="status"
      aria-live="polite"
      className={cn("flex-1 text-[0.8rem]", statusError ? "text-err" : "text-muted-foreground")}
    >
      {statusText}
    </div>
  );
}
