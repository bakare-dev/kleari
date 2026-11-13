import { useCallback, useEffect, useRef, useState } from "react";
import type { AddedFolder, DeveloperProgress, DeveloperScan } from "../../contracts/developer";
import { addDeveloperFolder, cancelDeveloperScan, startDeveloperScan } from "../../lib/developerIpc";
import { StorageError, type StorageErrorKind } from "../../lib/storageIpc";
import { storageErrorText } from "../storage/format";
import { idleDeveloperScan, type DeveloperScanState } from "./developerView";

export interface DeveloperLoaders {
  start: (onProgress: (progress: DeveloperProgress) => void) => Promise<DeveloperScan>;
  cancel: () => Promise<void>;
  addFolder: () => Promise<AddedFolder>;
}

const defaultLoaders: DeveloperLoaders = { start: startDeveloperScan, cancel: cancelDeveloperScan, addFolder: addDeveloperFolder };

function errorKind(error: unknown): StorageErrorKind {
  return error instanceof StorageError ? error.kind : "unknown";
}

/**
 * Runs the read-only Developer scan. Held at the app level so a running scan, its result and
 * the folders added this session survive leaving the page; nothing is stored.
 */
export function useDeveloperScan(loaders: DeveloperLoaders = defaultLoaders) {
  const [state, setState] = useState<DeveloperScanState>(idleDeveloperScan);
  const [addedFolders, setAddedFolders] = useState<readonly string[]>([]);
  const running = useRef(false);
  const mounted = useRef(true);

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

  const scan = useCallback(() => {
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
                : // A cancelled scan covers only part of the Mac; keep the last full one.
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

  /** Adds a folder through the macOS panel and scans again. Resolves to a message to show. */
  const addFolder = useCallback(async (): Promise<string | null> => {
    try {
      const added = await loaders.addFolder();
      if (added.status === "cancelled") {
        return null;
      }
      setAddedFolders((current) => (current.includes(added.name) ? current : [...current, added.name]));
      scan();
      return `Added ${added.name}. Scanning again.`;
    } catch (error) {
      return storageErrorText[errorKind(error)];
    }
  }, [loaders, scan]);

  return { state, addedFolders, scan, cancel, addFolder };
}
