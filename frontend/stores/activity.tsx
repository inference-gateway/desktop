import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";

const MAX_ACTIVITIES = 200;

export type ActivityStatus = "running" | "done" | "failed" | "skipped";

export interface ActivityEntry {
  id: number;
  project: string | null;
  action: string;
  status: ActivityStatus;
  message?: string;
}

function useActivityStore() {
  const [activities, setActivities] = useState<ActivityEntry[]>([]);
  const activityId = useRef(0);
  const [seenActivityId, setSeenActivityId] = useState(0);

  const logActivity = useCallback(
    (project: string | null, action: string, status: ActivityStatus, message?: string) => {
      const id = ++activityId.current;
      setActivities((prev) => {
        const i = prev.findIndex((a) => a.project === project && a.action === action && a.status === "running");
        if (i >= 0) return prev.map((a, j) => (j === i ? { ...a, id, status, message } : a));
        return [...prev.slice(-(MAX_ACTIVITIES - 1)), { id, project, action, status, message }];
      });
    },
    [],
  );
  const clearActivities = useCallback(() => setActivities([]), []);
  const markActivitiesSeen = useCallback(() => setSeenActivityId(activityId.current), []);

  return useMemo(
    () => ({ activities, seenActivityId, logActivity, clearActivities, markActivitiesSeen }),
    [activities, seenActivityId, logActivity, clearActivities, markActivitiesSeen],
  );
}

type ActivityStore = ReturnType<typeof useActivityStore>;

const ActivityContext = createContext<ActivityStore | null>(null);

export function ActivityProvider({ children }: { children: ReactNode }) {
  return <ActivityContext.Provider value={useActivityStore()}>{children}</ActivityContext.Provider>;
}

/** The log of background work per project (git actions, exports, imports) and
 * how far the person has read it. */
export function useActivity(): ActivityStore {
  const ctx = useContext(ActivityContext);
  if (!ctx) throw new Error("useActivity must be used within ActivityProvider");
  return ctx;
}
