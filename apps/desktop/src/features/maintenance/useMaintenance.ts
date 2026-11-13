import { useCallback, useEffect, useRef, useState } from "react";
import type { Task, TaskInfo, TaskOutcome } from "../../contracts/maintenance";
import type { StartupItems } from "../../contracts/systemPage";
import { getMaintenanceInfo, runMaintenanceTask, sendLaunchAgentToReview } from "../../lib/maintenanceIpc";
import { StorageError, type StorageErrorKind } from "../../lib/storageIpc";
import { fetchStartupItems } from "../../lib/systemPageIpc";
import { storageErrorText } from "../storage/format";

export interface MaintenanceLoaders {
  info: () => Promise<TaskInfo[]>;
  run: (task: Task) => Promise<TaskOutcome>;
  startup: () => Promise<StartupItems>;
  sendAgent: (label: string) => Promise<boolean>;
}

const defaultLoaders: MaintenanceLoaders = { info: getMaintenanceInfo, run: runMaintenanceTask, startup: fetchStartupItems, sendAgent: sendLaunchAgentToReview };

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
  const [sentAgents, setSentAgents] = useState<ReadonlySet<string>>(new Set());
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

  const sendAgent = useCallback(
    async (label: string): Promise<string> => {
      try {
        const sent = await loaders.sendAgent(label);
        if (!sent) {
          return "That login item is no longer in your LaunchAgents folder. Nothing was sent.";
        }
        setSentAgents((current) => new Set([...current, label]));
        onReviewChanged();
        return "Sent to review. It moves into quarantine only when you approve it, and stops starting from your next login.";
      } catch (failure) {
        return errorText(failure);
      }
    },
    [loaders, onReviewChanged],
  );

  return { info, startup, loading, running, last, error, sentAgents, refresh, run, sendAgent };
}

export type Maintenance = ReturnType<typeof useMaintenance>;
