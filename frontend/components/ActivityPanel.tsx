// Session log for background project actions (#256): one entry per action per
// project - branch cleanup, checkout + pull, /init, refresh - with the
// backend's summary or the error. In-memory, capped, collapsed by default so
// it stays out of the way until something runs or fails.
import { useEffect, useState } from "react";
import { ChevronDown, Trash2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { useDesktop, type ActivityStatus } from "@/store";

const DOT: Record<ActivityStatus, string> = {
  running: "animate-pulse bg-primary",
  done: "bg-emerald-500",
  failed: "bg-destructive",
  skipped: "bg-muted-foreground",
};

export function ActivityPanel() {
  const { activities, clearActivities, seenActivityId, markActivitiesSeen } = useDesktop();
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (open) markActivitiesSeen();
  }, [open, activities, markActivitiesSeen]);
  if (activities.length === 0) return null;
  const running = activities.filter((a) => a.status === "running").length;
  const failed = activities.filter((a) => a.status === "failed").length;
  const unread = open ? 0 : activities.filter((a) => a.id > seenActivityId).length;
  return (
    <div className="mx-5 mt-2 shrink-0 overflow-hidden rounded-md border border-border text-[0.85rem]">
      <div className="flex items-center gap-2 px-3 py-[0.35rem]">
        <button
          onClick={() => setOpen(!open)}
          aria-expanded={open}
          aria-label="Toggle activity panel"
          className="flex items-center gap-1.5 font-bold text-muted-foreground hover:text-foreground"
        >
          <ChevronDown size={14} className={cn("transition-transform", !open && "-rotate-90")} />
          Activity
        </button>
        {unread > 0 && (
          <span
            title={`${unread} new`}
            aria-label={`${unread} new activities`}
            className="min-w-[1.1rem] rounded-full bg-primary px-1 text-center text-[0.65rem] font-semibold text-primary-foreground"
          >
            {unread}
          </span>
        )}
        {running > 0 && (
          <span
            title={`${running} running`}
            aria-label={`${running} actions running`}
            className="min-w-[1.1rem] rounded-full bg-primary/15 px-1 text-center text-[0.65rem] font-semibold text-primary"
          >
            {running}
          </span>
        )}
        {failed > 0 && (
          <span
            title={`${failed} failed`}
            aria-label={`${failed} actions failed`}
            className="min-w-[1.1rem] rounded-full bg-destructive/15 px-1 text-center text-[0.65rem] font-semibold text-destructive"
          >
            {failed}
          </span>
        )}
        {open && (
          <button
            onClick={clearActivities}
            aria-label="Clear activity log"
            title="Clear the activity log"
            className="ml-auto rounded p-0.5 text-muted-foreground hover:bg-secondary hover:text-foreground"
          >
            <Trash2 size={13} />
          </button>
        )}
      </div>
      {open && (
        <ul className="max-h-40 overflow-y-auto border-t border-border">
          {[...activities].reverse().map((a) => (
            <li
              key={a.id}
              className="flex items-center gap-2 border-b border-border px-2 py-1 text-[0.78rem] last:border-b-0"
            >
              <span className={cn("h-[0.4rem] w-[0.4rem] shrink-0 rounded-full", DOT[a.status])} />
              <span className="shrink-0 text-foreground">{a.project ? `${a.project} - ${a.action}` : a.action}</span>
              {a.message && (
                <span
                  title={a.message}
                  className={cn(
                    "ml-auto min-w-0 truncate pl-2 text-[0.7rem]",
                    a.status === "failed" ? "text-err" : "text-muted-foreground",
                  )}
                >
                  {a.message}
                </span>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
