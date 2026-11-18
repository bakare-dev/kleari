import { useCallback, useEffect, useRef, useState } from "react";
import type { ActivityList } from "../../contracts/activity";
import { clearActivity, listActivity } from "../../lib/settingsIpc";
import { errorKind } from "../cleanup/useCleanupScan";
import { storageErrorText } from "../storage/format";

export interface ActivityLoaders {
  list: () => Promise<ActivityList>;
  clear: () => Promise<void>;
}

const defaultLoaders: ActivityLoaders = { list: listActivity, clear: clearActivity };

/**
 * The activity log, read at launch and held at the app level so the page shows the last list
 * at once while it reads the history again. `list` is null until the first read finishes. A
 * failure is kept as state, never shown as an empty log.
 */
export function useActivity(loaders: ActivityLoaders = defaultLoaders) {
  const [list, setList] = useState<ActivityList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const refresh = useCallback(() => {
    loaders.list().then(
      (next) => {
        if (mounted.current) {
          setList(next);
          setError(null);
        }
      },
      (failure: unknown) => {
        if (mounted.current) {
          setError(storageErrorText[errorKind(failure)]);
        }
      },
    );
  }, [loaders]);

  // Read at launch, so the Activity page already has the log when it's first opened; the
  // page reads it again on each visit to pick up what happened since.
  useEffect(() => {
    refresh();
  }, [refresh]);

  /** Deletes the log (quarantine is untouched). Resolves to a message when it failed. */
  const clear = useCallback(async (): Promise<string | null> => {
    try {
      await loaders.clear();
      refresh();
      return null;
    } catch (failure) {
      return storageErrorText[errorKind(failure)];
    }
  }, [loaders, refresh]);

  return { list, error, refresh, clear };
}

export type Activity = ReturnType<typeof useActivity>;
