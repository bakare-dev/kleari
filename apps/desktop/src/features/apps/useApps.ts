import { useCallback, useEffect, useRef, useState } from "react";
import type { AppsScan, RemoveOutcome } from "../../contracts/apps";
import type { CleanupPreview } from "../../contracts/cleanup";
import { cancelAppsScan, prepareAppItemsRemoval, removeApp, startAppsScan } from "../../lib/appsIpc";
import { StorageError, type StorageErrorKind } from "../../lib/storageIpc";
import { formatGb, plural } from "../../v2/ui/format";
import { storageErrorText } from "../storage/format";

export interface AppsLoaders {
  start: () => Promise<AppsScan>;
  cancel: () => Promise<void>;
  prepare: (ids: readonly string[]) => Promise<CleanupPreview>;
  remove: (id: string) => Promise<RemoveOutcome>;
}

const defaultLoaders: AppsLoaders = { start: startAppsScan, cancel: cancelAppsScan, prepare: prepareAppItemsRemoval, remove: removeApp };

export interface AppsState {
  running: boolean;
  result: AppsScan | null;
  /** The last scan was stopped; the result shown is the one before it. */
  stopped: boolean;
  error: string | null;
}

function errorText(error: unknown): string {
  const kind: StorageErrorKind = error instanceof StorageError ? error.kind : "unknown";
  return storageErrorText[kind];
}

/** What a removal outcome means, in words; `removed` says how much the app left behind. */
export function removeMessage(name: string, outcome: RemoveOutcome): { text: string; ok: boolean } {
  switch (outcome.status) {
    case "removed":
      return {
        ok: true,
        text:
          outcome.leftovers > 0
            ? `Moved ${name} to the Trash. It left ${plural(outcome.leftovers, "item")} behind (${formatGb(outcome.leftoverBytes / 1e9)}).`
            : `Moved ${name} to the Trash. It left nothing behind in your Library.`,
      };
    case "running":
      return { ok: false, text: `${name} is open. Quit it, then remove it.` };
    case "refused":
      return { ok: false, text: `${name} is part of macOS or isn’t in an Applications folder, so Kleari doesn’t remove it.` };
    case "changed":
      return { ok: false, text: `${name} changed since the scan. Scan again.` };
    case "failed":
      return {
        ok: false,
        text: `macOS didn’t move ${name}. Allow Kleari in System Settings › Privacy & Security › App Management, or remove an app an administrator installed in Finder.`,
      };
  }
}

/**
 * The Apps page's read-only scan, removing leftovers and installers, and app removal. Held at the
 * app level so a running scan and its result survive leaving the page; nothing is stored.
 */
export function useApps(onReviewChanged: () => void, loaders: AppsLoaders = defaultLoaders) {
  const [state, setState] = useState<AppsState>({ running: false, result: null, stopped: false, error: null });
  const running = useRef(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (running.current) {
        loaders.cancel().catch(() => {
          // Deliberate: the window is going away and the scan ends with it either way.
        });
      }
    };
  }, [loaders]);

  const scan = useCallback(() => {
    if (running.current) {
      return;
    }
    running.current = true;
    setState((current) => ({ ...current, running: true, stopped: false, error: null }));
    loaders.start().then(
      (result) => {
        running.current = false;
        if (mounted.current) {
          setState((current) =>
            result.status === "completed"
              ? { running: false, result, stopped: false, error: null }
              : // A stopped scan covers only part of the Mac; keep the last full one.
                { ...current, running: false, stopped: true },
          );
        }
      },
      (error: unknown) => {
        running.current = false;
        if (mounted.current) {
          setState((current) => ({ ...current, running: false, error: errorText(error) }));
        }
      },
    );
  }, [loaders]);

  const cancel = useCallback(() => {
    loaders.cancel().catch((error: unknown) => {
      if (mounted.current) {
        setState((current) => ({ ...current, error: errorText(error) }));
      }
    });
  }, [loaders]);

  /** Prepares leftovers or installers for removal on this page (the confirm sheet shows it). */
  const prepare = useCallback((ids: readonly string[]) => loaders.prepare(ids), [loaders]);

  /** After items moved: scans again, and lets Cleanup's review know. */
  const refresh = useCallback(() => {
    onReviewChanged();
    scan();
  }, [onReviewChanged, scan]);

  /** Removes an app, then scans again so the list shows what is installed now. `leftovers` is
   * the preview of what it left behind, for "Also remove what it left?". */
  const remove = useCallback(
    async (id: string, name: string): Promise<{ text: string; ok: boolean; leftovers: CleanupPreview | null }> => {
      try {
        const outcome = await loaders.remove(id);
        if (outcome.status === "removed") {
          refresh();
        }
        const leftovers = outcome.status === "removed" && outcome.preview.items.some((item) => item.state === "planned") ? outcome.preview : null;
        return { ...removeMessage(name, outcome), leftovers };
      } catch (error) {
        return { ok: false, text: errorText(error), leftovers: null };
      }
    },
    [loaders, refresh],
  );

  return { state, scan, cancel, prepare, refresh, remove };
}

export type Apps = ReturnType<typeof useApps>;
