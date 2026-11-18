import { useCallback, useEffect, useRef, useState } from "react";
import type { Listing, ListingProgress } from "../../contracts/storageExplorer";
import type { CleanupPreview } from "../../contracts/cleanup";
import { cancelStorageListing, listStorageFolder, prepareStorageItemsRemoval } from "../../lib/storageExplorerIpc";
import { StorageError, type StorageErrorKind } from "../../lib/storageIpc";
import { storageErrorText } from "./format";

export interface ExplorerLoaders {
  list: (id: string | null, onProgress: (progress: ListingProgress) => void) => Promise<Listing | null>;
  cancel: () => Promise<void>;
  prepare: (ids: readonly string[]) => Promise<CleanupPreview>;
}

const defaultLoaders: ExplorerLoaders = { list: listStorageFolder, cancel: cancelStorageListing, prepare: prepareStorageItemsRemoval };

export interface ExplorerState {
  /** The folder shown; kept while the next one is measured. */
  listing: Listing | null;
  loading: boolean;
  progress: ListingProgress | null;
  error: string | null;
}

function errorText(error: unknown): string {
  const kind: StorageErrorKind = error instanceof StorageError ? error.kind : "unknown";
  return storageErrorText[kind];
}

/**
 * The Storage page's Browse view: the folder shown, opening another, and removing what you
 * chose (its confirm sheet). Held at the app level so the folder you were in is still there
 * when you come back; nothing is stored.
 */
export function useStorageExplorer(onReviewChanged: () => void, loaders: ExplorerLoaders = defaultLoaders) {
  const [state, setState] = useState<ExplorerState>({ listing: null, loading: false, progress: null, error: null });
  const request = useRef(0);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  /** Opens a folder (`null`: Home). A newer request replaces an older one still running. */
  const open = useCallback(
    (id: string | null) => {
      const mine = ++request.current;
      setState((current) => ({ ...current, loading: true, progress: null, error: null }));
      loaders
        .list(id, (progress) => {
          if (mounted.current && request.current === mine) {
            setState((current) => ({ ...current, progress }));
          }
        })
        .then(
          (listing) => {
            if (!mounted.current || request.current !== mine) {
              return;
            }
            if (listing === null) {
              // Forgotten since (too many folders opened): start again from Home.
              setState((current) => ({ ...current, loading: false, error: "That folder is no longer known. Showing Home." }));
              if (id !== null) {
                open(null);
              }
              return;
            }
            setState((current) =>
              listing.status === "cancelled"
                ? { ...current, loading: false, progress: null }
                : { listing, loading: false, progress: null, error: null },
            );
          },
          (error: unknown) => {
            if (mounted.current && request.current === mine) {
              setState((current) => ({ ...current, loading: false, progress: null, error: errorText(error) }));
            }
          },
        );
    },
    [loaders],
  );

  /** Measures the folder shown again (after something in it moved), if one is shown. */
  const listingId = state.listing?.id ?? null;
  const reload = useCallback(() => {
    if (listingId !== null) {
      open(listingId);
    }
  }, [listingId, open]);

  const cancel = useCallback(() => {
    loaders.cancel().catch((error: unknown) => {
      if (mounted.current) {
        setState((current) => ({ ...current, error: errorText(error) }));
      }
    });
  }, [loaders]);

  /** Prepares what you chose for removal here (the confirm sheet shows it). */
  const prepare = useCallback((ids: readonly string[]) => loaders.prepare(ids), [loaders]);

  /** After something moved: measures the folder shown again, and lets Cleanup's review know. */
  const removed = useCallback(() => {
    onReviewChanged();
    reload();
  }, [onReviewChanged, reload]);

  return { state, open, reload, cancel, prepare, removed };
}

export type StorageExplorer = ReturnType<typeof useStorageExplorer>;
