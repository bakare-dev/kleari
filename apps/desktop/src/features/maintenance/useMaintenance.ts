import { useCallback, useEffect, useRef, useState } from "react";
import type { Task, TaskInfo, TaskOutcome } from "../../contracts/maintenance";
import type { StartupItems } from "../../contracts/systemPage";
import type { CleanupPreview } from "../../contracts/cleanup";
import { getMaintenanceInfo, prepareLaunchAgentRemoval, runMaintenanceTask } from "../../lib/maintenanceIpc";
import { StorageError, type StorageErrorKind } from "../../lib/storageIpc";
import { fetchStartupItems } from "../../lib/systemPageIpc";
import { storageErrorText } from "../storage/format";

export interface MaintenanceLoaders {
  info: () => Promise<TaskInfo[]>;
  run: (task: Task) => Promise<TaskOutcome>;
  startup: () => Promise<StartupItems>;
  prepareAgent: (label: string) => Promise<CleanupPreview>;
}

const defaultLoaders: MaintenanceLoaders = { info: getMaintenanceInfo, run: runMaintenanceTask, startup: fetchStartupItems, prepareAgent: prepareLaunchAgentRemoval };

function errorText(error: unknown): string {
  const kind: StorageErrorKind = error instanceof StorageError ? error.kind : "unknown";
  return storageErrorText[kind];
}

/**
 * The Maintenance page: what each task concerns, the task running (one at a time), and login
 * items. Held at the app level so a running task's result still shows after leaving the page.
 */
export function useMaintenance(onReviewChanged: () => void, loaders: MaintenanceLoaders = defaultLoaders) {
  const [info, setInfo] = useState<TaskInfo[] | null>(null);
  const [startup, setStartup] = useState<StartupItems | null>(null);
  const [loading, setLoading] = useState(false);
  const [running, setRunning] = useState<Task | null>(null);
  const [last, setLast] = useState<{ task: Task; outcome: TaskOutcome } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  /** Measures again; called when the page opens and after each task. */
  const refresh = useCallback(() => {
    setLoading(true);
    Promise.allSettled([loaders.info(), loaders.startup()]).then(([nextInfo, nextStartup]) => {
      if (!mounted.current) {
        return;
      }
      setLoading(false);
      if (nextInfo.status === "fulfilled") {
        setInfo(nextInfo.value);
        setError(null);
      } else {
        setError(errorText(nextInfo.reason));
      }
      if (nextStartup.status === "fulfilled") {
        setStartup(nextStartup.value);
      }
    });
  }, [loaders]);

  const run = useCallback(
    async (task: Task): Promise<TaskOutcome | null> => {
      setRunning(task);
      try {
        const outcome = await loaders.run(task);
        if (mounted.current) {
          setLast({ task, outcome });
          setRunning(null);
          refresh();
        }
        return outcome;
      } catch (failure) {
        if (mounted.current) {
          setRunning(null);
          setError(errorText(failure));
        }
        return null;
      }
    },
    [loaders, refresh],
  );

  /** Prepares your launch agent with this label for removal here (the confirm sheet shows it;
   * empty if it is no longer in your LaunchAgents folder). */
  const prepareAgent = useCallback((label: string) => loaders.prepareAgent(label), [loaders]);

  /** After something moved: measures again, and lets Cleanup's review know. */
  const removed = useCallback(() => {
    onReviewChanged();
    refresh();
  }, [onReviewChanged, refresh]);

  return { info, startup, loading, running, last, error, refresh, run, prepareAgent, removed };
}

export type Maintenance = ReturnType<typeof useMaintenance>;
