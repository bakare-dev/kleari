import { useCallback, useEffect, useRef, useState } from "react";
import type { AppsScan, RemoveOutcome } from "../../contracts/apps";
import { cancelAppsScan, removeApp, sendAppItemsToReview, startAppsScan } from "../../lib/appsIpc";
import { StorageError, type StorageErrorKind } from "../../lib/storageIpc";
import { formatGb, plural } from "../../v2/ui/format";
import { storageErrorText } from "../storage/format";

export interface AppsLoaders {
  start: () => Promise<AppsScan>;
  cancel: () => Promise<void>;
  send: (ids: readonly string[]) => Promise<string[]>;
  remove: (id: string) => Promise<RemoveOutcome>;
}

const defaultLoaders: AppsLoaders = { start: startAppsScan, cancel: cancelAppsScan, send: sendAppItemsToReview, remove: removeApp };

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

/** What a removal outcome means, in words; `removed` says what went to the review. */
export function removeMessage(name: string, outcome: RemoveOutcome): { text: string; ok: boolean } {
  switch (outcome.status) {
    case "removed":
      return {
        ok: true,
        text:
          outcome.leftovers > 0
            ? `Moved ${name} to the Trash. ${plural(outcome.leftovers, "item")} it left behind (${formatGb(outcome.leftoverBytes / 1e9)}) are in Cleanup’s review. Nothing else has moved.`
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
 * The Apps page's read-only scan, what was sent to the review and app removal. Held at the
 * app level so a running scan and its result survive leaving the page; nothing is stored.
 */
export function useApps(onReviewChanged: () => void, loaders: AppsLoaders = defaultLoaders) {
  const [state, setState] = useState<AppsState>({ running: false, result: null, stopped: false, error: null });
  const [sent, setSent] = useState<ReadonlySet<string>>(new Set());
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
          if (result.status === "completed") {
            setSent(new Set());
          }
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

  /** Sends leftovers or installers to Cleanup's review. Resolves to a message. */
  const send = useCallback(
    async (ids: readonly string[]): Promise<string> => {
      try {
        const done = await loaders.send(ids);
        setSent((current) => new Set([...current, ...done]));
        onReviewChanged();
        return done.length === ids.length
          ? `Sent ${plural(done.length, "item")} to review. Nothing has moved.`
          : `Sent ${done.length} of ${ids.length} to review. Scan again to send the rest. Nothing has moved.`;
      } catch (error) {
        return errorText(error);
      }
    },
    [loaders, onReviewChanged],
  );

  /** Removes an app, then scans again so the list shows what is installed now. */
  const remove = useCallback(
    async (id: string, name: string): Promise<{ text: string; ok: boolean }> => {
      try {
        const outcome = await loaders.remove(id);
        if (outcome.status === "removed") {
          onReviewChanged();
          scan();
        }
        return removeMessage(name, outcome);
      } catch (error) {
        return { ok: false, text: errorText(error) };
      }
    },
    [loaders, onReviewChanged, scan],
  );

  return { state, sent, scan, cancel, send, remove };
}

export type Apps = ReturnType<typeof useApps>;
