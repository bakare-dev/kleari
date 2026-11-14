import { useCallback, useEffect, useRef, useState } from "react";
import type { FilesOptions, FilesScan, Tool } from "../../contracts/files";
import { cancelFilesScan, sendFilesToReview, startFilesScan } from "../../lib/filesIpc";
import { StorageError, type StorageErrorKind } from "../../lib/storageIpc";
import { plural } from "../../v2/ui/format";
import { storageErrorText } from "../storage/format";

export interface FilesLoaders {
  start: (tool: Tool, options: FilesOptions) => Promise<FilesScan>;
  cancel: () => Promise<void>;
  send: (ids: readonly string[]) => Promise<string[]>;
}

const defaultLoaders: FilesLoaders = { start: startFilesScan, cancel: cancelFilesScan, send: sendFilesToReview };

function errorText(error: unknown): string {
  const kind: StorageErrorKind = error instanceof StorageError ? error.kind : "unknown";
  return storageErrorText[kind];
}

export interface FilesState {
  /** The last completed result of each tool. */
  results: Partial<Record<Tool, FilesScan>>;
  running: Tool | null;
  error: string | null;
  stopped: boolean;
}

/**
 * The Files page's tools: one runs at a time, each keeps its last result, and what you choose
 * goes to Cleanup's review. Held at the app level so a search survives leaving the page.
 */
export function useFiles(onReviewChanged: () => void, loaders: FilesLoaders = defaultLoaders) {
  const [state, setState] = useState<FilesState>({ results: {}, running: null, error: null, stopped: false });
  const [sent, setSent] = useState<ReadonlySet<string>>(new Set());
  const running = useRef(false);
  const mounted = useRef(true);
  /** The last search asked for, so it can be run again after its files moved. */
  const last = useRef<{ tool: Tool; options: FilesOptions } | null>(null);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (running.current) {
        loaders.cancel().catch(() => {
          // Deliberate: the window is going away and the search ends with it either way.
        });
      }
    };
  }, [loaders]);

  const run = useCallback(
    (tool: Tool, options: FilesOptions) => {
      if (running.current) {
        return;
      }
      running.current = true;
      last.current = { tool, options };
      setState((current) => ({ ...current, running: tool, error: null, stopped: false }));
      loaders.start(tool, options).then(
        (result) => {
          running.current = false;
          if (mounted.current) {
            setState((current) =>
              result.status === "completed"
                ? { ...current, running: null, results: { ...current.results, [tool]: result } }
                : { ...current, running: null, stopped: true },
            );
          }
        },
        (error: unknown) => {
          running.current = false;
          if (mounted.current) {
            setState((current) => ({ ...current, running: null, error: errorText(error) }));
          }
        },
      );
    },
    [loaders],
  );

  const cancel = useCallback(() => {
    loaders.cancel().catch((error: unknown) => {
      if (mounted.current) {
        setState((current) => ({ ...current, error: errorText(error) }));
      }
    });
  }, [loaders]);

  const send = useCallback(
    async (ids: readonly string[]): Promise<string> => {
      try {
        const done = await loaders.send(ids);
        if (done.length === ids.length) {
          setSent((current) => new Set([...current, ...ids]));
        }
        onReviewChanged();
        return done.length === ids.length
          ? `Sent ${plural(done.length, "item")} to review. Nothing has moved.`
          : `Sent ${done.length} of ${ids.length} to review; one copy of each group stays, and changed items were left out. Nothing has moved.`;
      } catch (error) {
        return errorText(error);
      }
    },
    [loaders, onReviewChanged],
  );

  /** Runs the last search again (after what it found was moved), if there was one. */
  const rerun = useCallback(() => {
    if (last.current) {
      setSent(new Set());
      run(last.current.tool, last.current.options);
    }
  }, [run]);

  return { state, sent, run, rerun, cancel, send };
}

export type Files = ReturnType<typeof useFiles>;
