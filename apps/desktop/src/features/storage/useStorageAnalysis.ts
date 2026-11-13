import { useCallback, useEffect, useRef, useState } from "react";
import type { AnalysisProgress, StorageAnalysisResult, StorageOverview } from "../../contracts/storageAnalysis";
import {
  cancelStorageAnalysis,
  fetchStorageOverview,
  startStorageAnalysis,
  StorageError,
  type StorageErrorKind,
} from "../../lib/storageIpc";
import { openSystemSettings } from "../../lib/systemPageIpc";
import { toErrorKind, useDocumentVisible } from "../overview/useSystemMetrics";
import type { Loadable } from "../system/systemView";
import { idleAnalysis, type AnalysisState } from "./storageAnalysisView";

export interface AnalysisLoaders {
  start: (onProgress: (progress: AnalysisProgress) => void) => Promise<StorageAnalysisResult>;
  cancel: () => Promise<void>;
}

const defaultAnalysisLoaders: AnalysisLoaders = { start: startStorageAnalysis, cancel: cancelStorageAnalysis };

function errorKind(error: unknown): StorageErrorKind {
  return error instanceof StorageError ? error.kind : "unknown";
}

/**
 * Runs the read-only storage analysis. Held at the app level so a running analysis and the
 * last result survive leaving the Storage page; nothing is stored beyond this session.
 */
export function useStorageAnalysis(loaders: AnalysisLoaders = defaultAnalysisLoaders) {
  const [state, setState] = useState<AnalysisState>(idleAnalysis);
  const running = useRef(false);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (running.current) {
        // Nothing will show the result, so stop reading the disk.
        loaders.cancel().catch(() => {
          // Deliberate: the window is going away and the analysis ends with it either way.
        });
      }
    };
  }, [loaders]);

  const analyze = useCallback(() => {
    if (running.current) {
      return;
    }
    running.current = true;
    setState((current) => ({ ...current, running: true, progress: null, stopped: false, error: null }));
    loaders
      .start((progress) => {
        if (mounted.current) {
          setState((current) => (current.running ? { ...current, progress } : current));
        }
      })
      .then(
        (result) => {
          running.current = false;
          if (mounted.current) {
            setState((current) =>
              result.status === "completed"
                ? { running: false, progress: null, result, stopped: false, error: null }
                : // A cancelled analysis covers only part of the disk; keep the last full one.
                  { ...current, running: false, progress: null, stopped: true },
            );
          }
        },
        (error: unknown) => {
          running.current = false;
          if (mounted.current) {
            setState((current) => ({ ...current, running: false, progress: null, error: errorKind(error) }));
          }
        },
      );
  }, [loaders]);

  const cancel = useCallback(() => {
    loaders.cancel().catch((error: unknown) => {
      if (mounted.current) {
        setState((current) => ({ ...current, error: errorKind(error) }));
      }
    });
  }, [loaders]);

  return { state, analyze, cancel };
}

/**
 * Capacity, managed space and Full Disk Access. Read on each visit, when the window comes
 * back to the front (Full Disk Access may have been turned on in System Settings), and after
 * each analysis.
 */
export function useStorageOverview(refreshKey: unknown, load: () => Promise<StorageOverview> = fetchStorageOverview) {
  const [state, setState] = useState<Loadable<StorageOverview>>({ status: "loading" });
  const visible = useDocumentVisible();

  useEffect(() => {
    if (!visible) {
      return;
    }
    let cancelled = false;
    load().then(
      (value) => !cancelled && setState({ status: "ready", value }),
      (error: unknown) =>
        !cancelled &&
        setState((current) => ({
          status: "error",
          error: toErrorKind(error),
          last: current.status === "ready" ? current.value : current.status === "error" ? current.last : null,
        })),
    );
    return () => {
      cancelled = true;
    };
  }, [load, visible, refreshKey]);

  return state;
}

export function openFullDiskAccessSettings(): Promise<boolean> {
  return openSystemSettings("fullDiskAccess");
}
