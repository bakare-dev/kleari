import { useCallback, useEffect, useRef, useState } from "react";
import type { AddedCleanupFolder, DeleteProgress, DeleteSummary, QuarantineList, RestoreResult } from "../../contracts/cleanup";
import { chooseRestoreFolder, deleteQuarantineItems, listQuarantine, restoreQuarantineItems, revealQuarantineItem, stopQuarantineDelete } from "../../lib/cleanupIpc";
import { chooseFolderHint, type DeleteJob, type QuarantineItem, type RestoreOutcome, type RestoreReason } from "../../v2/screens/quarantine/types";
import { storageErrorText } from "../storage/format";
import { errorKind } from "./useCleanupScan";

export interface QuarantineLoaders {
  list: () => Promise<QuarantineList>;
  restore: (ids: readonly string[]) => Promise<RestoreResult[]>;
  remove: (ids: readonly string[], onProgress: (progress: DeleteProgress) => void) => Promise<DeleteSummary>;
  stop: () => Promise<void>;
  reveal: (id: string, withOriginal: boolean) => Promise<void>;
  /** Opens the macOS folder panel; the chosen folder allows restores into it this session,
   * and is never scanned. */
  chooseFolder: () => Promise<AddedCleanupFolder>;
}

const defaultLoaders: QuarantineLoaders = {
  list: listQuarantine,
  restore: restoreQuarantineItems,
  remove: deleteQuarantineItems,
  stop: stopQuarantineDelete,
  reveal: revealQuarantineItem,
  chooseFolder: chooseRestoreFolder,
};

/** What the page says about a restore, from the backend's outcome for each item. */
export function restoreOutcome(items: readonly QuarantineItem[], results: readonly RestoreResult[]): RestoreOutcome {
  const outcomeOf = (item: QuarantineItem) => results.find((result) => result.id === item.id)?.outcome;
  const restored = items.filter((item) => outcomeOf(item) === "restored").length;
  const unconfirmed = items.filter((item) => outcomeOf(item) === "unconfirmed");
  if (unconfirmed.length) {
    return { kind: "unconfirmed", restored, unconfirmed };
  }
  const blocked = items.filter((item) => outcomeOf(item) !== "restored");
  const reasons: Record<string, RestoreReason> = {};
  for (const item of blocked) {
    const outcome = outcomeOf(item);
    // An item the backend didn't answer for is reported as a failure, never guessed.
    reasons[item.id] = outcome === undefined || outcome === "restored" || outcome === "unconfirmed" ? "failed" : outcome;
  }
  return blocked.length ? { kind: "blocked", restored, blocked, reasons } : { kind: "restored", count: restored };
}

/**
 * What is held in quarantine, restoring items and permanently deleting them (after the
 * page's separate confirmation). Held at the app level so a running delete survives leaving
 * the page.
 */
export function useQuarantine(loaders: QuarantineLoaders = defaultLoaders) {
  const [list, setList] = useState<QuarantineList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [restored, setRestored] = useState<ReadonlySet<string>>(new Set());
  const [job, setJob] = useState<DeleteJob | null>(null);
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

  const restore = useCallback(
    async (items: readonly QuarantineItem[]): Promise<RestoreOutcome> => {
      try {
        const results = await loaders.restore(items.map((item) => item.id));
        const back = results.filter((result) => result.outcome === "restored").map((result) => result.id);
        setRestored((current) => new Set([...current, ...back]));
        refresh();
        return restoreOutcome(items, results);
      } catch (failure) {
        return { kind: "failed", message: storageErrorText[errorKind(failure)] };
      }
    },
    [loaders, refresh],
  );

  /** Permanently deletes items one at a time. The page has already confirmed it. */
  const startDelete = useCallback(
    (items: readonly QuarantineItem[]) => {
      setJob({ items, done: 0, failed: new Set(), stopRequested: false, finished: false });
      loaders
        .remove(
          items.map((item) => item.id),
          (progress) => {
            if (mounted.current && progress.state !== "deleting") {
              setJob((current) =>
                current
                  ? {
                      ...current,
                      done: current.done + 1,
                      failed: progress.state === "failed" ? new Set([...current.failed, progress.id]) : current.failed,
                    }
                  : current,
              );
            }
          },
        )
        .then(
          (summary) => {
            if (mounted.current) {
              setJob((current) =>
                current ? { ...current, done: summary.deleted.length + summary.failed.length, failed: new Set(summary.failed), finished: true } : current,
              );
              refresh();
            }
          },
          (failure: unknown) => {
            if (mounted.current) {
              setError(storageErrorText[errorKind(failure)]);
              setJob((current) => (current ? { ...current, finished: true } : current));
              refresh();
            }
          },
        );
    },
    [loaders, refresh],
  );

  const stopAfterCurrent = useCallback(() => {
    setJob((current) => (current && !current.finished ? { ...current, stopRequested: true } : current));
    loaders.stop().catch((failure: unknown) => {
      if (mounted.current) {
        setError(storageErrorText[errorKind(failure)]);
      }
    });
  }, [loaders]);

  const closeJob = useCallback(() => {
    setJob((current) => (current?.finished ? null : current));
  }, []);

  /** Resolves to a message when Finder couldn't show it. */
  const reveal = useCallback(
    async (item: QuarantineItem, withOriginal: boolean): Promise<string | null> => {
      try {
        await loaders.reveal(item.id, withOriginal);
        return null;
      } catch (failure) {
        return storageErrorText[errorKind(failure)];
      }
    },
    [loaders],
  );

  /**
   * Asks for the folder an item came from, then reads the list again. Resolves to a message
   * when the item still can't be restored (another folder was chosen, or the panel failed),
   * `null` when it can or the panel was cancelled.
   */
  const chooseFolder = useCallback(
    async (item: QuarantineItem): Promise<string | null> => {
      try {
        const added = await loaders.chooseFolder();
        if (added.status === "cancelled") {
          return null;
        }
        const next = await loaders.list();
        if (mounted.current) {
          setList(next);
          setError(null);
        }
        const still =
          next.status === "available" &&
          next.batches.some((batch) => batch.items.some((entry) => entry.id === item.id && entry.chooseFolder));
        return still ? `That folder doesn’t hold ${item.name}. ${chooseFolderHint(item)}` : null;
      } catch (failure) {
        return storageErrorText[errorKind(failure)];
      }
    },
    [loaders],
  );

  return { list, error, restored, job, refresh, restore, startDelete, stopAfterCurrent, closeJob, reveal, chooseFolder };
}
