import { useCallback, useEffect, useRef, useState } from "react";
import type { AddedCleanupFolder, CleanupProgress, CleanupScanResult, FolderAccess, ProtectedFolder, ScanMode, ScanOptionsInput, ScopeFolder } from "../../contracts/cleanup";
import { addCleanupFolder, cancelCleanupScan, getCleanupScope, requestFolderAccess, startCleanupScan } from "../../lib/cleanupIpc";
import { StorageError, type StorageErrorKind } from "../../lib/storageIpc";
import { defaultScanOptions, type CleanupScanState, type ScanOptions, type ScanPreset } from "../../v2/screens/cleanup/types";
import { storageErrorText } from "../storage/format";
import { scanOptionsInput } from "./cleanupView";

export interface CleanupScanLoaders {
  scope: () => Promise<ScopeFolder[]>;
  requestAccess: (folder: ProtectedFolder) => Promise<FolderAccess>;
  addFolder: () => Promise<AddedCleanupFolder>;
  start: (mode: ScanMode, options: ScanOptionsInput, onProgress: (progress: CleanupProgress) => void) => Promise<CleanupScanResult>;
  cancel: () => Promise<void>;
}

const defaultLoaders: CleanupScanLoaders = {
  scope: getCleanupScope,
  requestAccess: requestFolderAccess,
  addFolder: addCleanupFolder,
  start: startCleanupScan,
  cancel: cancelCleanupScan,
};

export function errorKind(error: unknown): StorageErrorKind {
  return error instanceof StorageError ? error.kind : "unknown";
}

export interface CleanupScanView extends CleanupScanState {
  /** The options the running or last scan started with. */
  options: ScanOptions;
  result: CleanupScanResult | null;
  error: string | null;
}

/**
 * The Cleanup page's read-only scan and the folders it reads. Held at the app level so a
 * running scan and its result survive leaving the page. `onFinished` runs after each scan so
 * the review can reload.
 */
export function useCleanupScan(onFinished: () => void, loaders: CleanupScanLoaders = defaultLoaders) {
  const [scan, setScan] = useState<CleanupScanView>({ mode: "standard", phase: "idle", step: 1, options: defaultScanOptions, result: null, error: null });
  const [scope, setScope] = useState<ScopeFolder[] | null>(null);
  const [scopeError, setScopeError] = useState<string | null>(null);
  const [preset, setPreset] = useState<ScanPreset | null>(null);
  // The options menu, kept while you move between pages.
  const [options, setOptions] = useState<ScanOptions>(defaultScanOptions);
  const running = useRef(false);
  const mounted = useRef(true);
  const finished = useRef(onFinished);
  useEffect(() => {
    finished.current = onFinished;
  }, [onFinished]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (running.current) {
        // Nothing will show the result, so stop reading the disk.
        loaders.cancel().catch(() => {
          // Deliberate: the window is going away and the scan ends with it either way.
        });
      }
    };
  }, [loaders]);

  const refreshScope = useCallback(() => {
    loaders.scope().then(
      (folders) => {
        if (mounted.current) {
          setScope(folders);
          setScopeError(null);
        }
      },
      (error: unknown) => {
        if (mounted.current) {
          setScopeError(storageErrorText[errorKind(error)]);
        }
      },
    );
  }, [loaders]);

  const start = useCallback(
    (mode: ScanMode, started: ScanOptions) => {
      if (running.current) {
        return;
      }
      running.current = true;
      setPreset(null);
      setScan({ mode, phase: "scanning", step: 1, options: started, result: null, error: null, found: [], startedAtMs: Date.now() });
      loaders
        .start(mode, scanOptionsInput(started), (progress) => {
          if (mounted.current) {
            setScan((current) => {
              if (current.phase !== "scanning") {
                return current;
              }
              // Each step's latest count; earlier steps keep their final one.
              const found = [...(current.found ?? [])];
              found[progress.step] = { items: progress.items, bytes: progress.bytes };
              return { ...current, step: progress.step + 1, found };
            });
          }
        })
        .then(
          (result) => {
            running.current = false;
            if (mounted.current) {
              setScan({
                mode,
                phase: result.cancelled ? "cancelled" : "done",
                step: result.steps.length,
                options: started,
                result,
                error: null,
                found: result.steps.map((step) => ({ items: step.items, bytes: step.bytes })),
                startedAtMs: null,
              });
              finished.current();
            }
          },
          (error: unknown) => {
            running.current = false;
            if (mounted.current) {
              setScan({ mode, phase: "idle", step: 1, options: started, result: null, error: storageErrorText[errorKind(error)] });
            }
          },
        );
    },
    [loaders],
  );

  const cancel = useCallback(() => {
    loaders.cancel().catch((error: unknown) => {
      if (mounted.current) {
        setScan((current) => ({ ...current, error: storageErrorText[errorKind(error)] }));
      }
    });
  }, [loaders]);

  /** Back to the start card for `mode`; the review keeps the last scan's findings. */
  const reset = useCallback((mode: ScanMode) => {
    if (!running.current) {
      setScan((current) => ({ ...current, mode, phase: "idle", step: 1, error: null }));
    }
  }, []);

  const requestAccess = useCallback(
    async (folder: ProtectedFolder): Promise<FolderAccess | null> => {
      try {
        const access = await loaders.requestAccess(folder);
        refreshScope();
        return access;
      } catch (error) {
        setScopeError(storageErrorText[errorKind(error)]);
        return null;
      }
    },
    [loaders, refreshScope],
  );

  /** Adds a folder through the macOS panel. Resolves to a message to show. */
  const addFolder = useCallback(async (): Promise<string | null> => {
    try {
      const added = await loaders.addFolder();
      if (added.status === "cancelled") {
        return null;
      }
      refreshScope();
      return `Added ${added.path} with read-only access`;
    } catch (error) {
      return storageErrorText[errorKind(error)];
    }
  }, [loaders, refreshScope]);

  /** Opens Cleanup with a mode and options chosen (Storage's Scan in Cleanup). Never scans. */
  const prepare = useCallback((next: ScanPreset) => {
    setPreset(next);
    setOptions((current) => ({ ...current, ...next.options }));
    if (!running.current) {
      setScan((current) => ({ ...current, mode: next.mode, phase: "idle", step: 1 }));
    }
  }, []);

  /**
   * Starts from the defaults saved in Settings (at launch, and when they are changed there).
   * A running scan, a shown result and Storage's Scan in Cleanup keep their own choice.
   */
  const applyDefaults = useCallback((mode: ScanMode, next: ScanOptions) => {
    if (running.current) {
      return;
    }
    setOptions(next);
    setScan((current) => (current.phase === "idle" && current.result === null ? { ...current, mode, options: next } : current));
  }, []);

  return { scan, scope, scopeError, preset, options, setOptions, refreshScope, start, cancel, reset, requestAccess, addFolder, prepare, applyDefaults };
}
