import { useCallback, useEffect, useRef, useState } from "react";
import type { FilesOptions, FilesScan, Tool } from "../../contracts/files";
import { cancelFilesScan, prepareFilesRemoval, startFilesScan } from "../../lib/filesIpc";
import type { CleanupPreview } from "../../contracts/cleanup";
import { StorageError, type StorageErrorKind } from "../../lib/storageIpc";
import { storageErrorText } from "../storage/format";

export interface FilesLoaders {
  start: (tool: Tool, options: FilesOptions) => Promise<FilesScan>;
  cancel: () => Promise<void>;
  prepare: (ids: readonly string[]) => Promise<CleanupPreview>;
}

const defaultLoaders: FilesLoaders = { start: startFilesScan, cancel: cancelFilesScan, prepare: prepareFilesRemoval };

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
 * is removed on the page (its confirm sheet). Held at the app level so a search survives
 * leaving the page.
 */
export function useFiles(onReviewChanged: () => void, loaders: FilesLoaders = defaultLoaders) {
  const [state, setState] = useState<FilesState>({ results: {}, running: null, error: null, stopped: false });
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

  /** Prepares what you chose for removal on this page (the confirm sheet shows it). */
  const prepare = useCallback((ids: readonly string[]) => loaders.prepare(ids), [loaders]);

  /** Runs the last search again (after what it found was moved), if there was one, and lets
   * Cleanup's review know files are gone. */
  const rerun = useCallback(() => {
    onReviewChanged();
    if (last.current) {
      run(last.current.tool, last.current.options);
    }
  }, [run, onReviewChanged]);

  return { state, run, rerun, cancel, prepare };
}

export type Files = ReturnType<typeof useFiles>;
