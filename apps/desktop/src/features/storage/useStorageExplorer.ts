import { useCallback, useEffect, useRef, useState } from "react";
import type { Listing, ListingProgress } from "../../contracts/storageExplorer";
import { cancelStorageListing, listStorageFolder, sendStorageItemsToReview } from "../../lib/storageExplorerIpc";
import { StorageError, type StorageErrorKind } from "../../lib/storageIpc";
import { plural } from "../../v2/ui/format";
import { storageErrorText } from "./format";

export interface ExplorerLoaders {
  list: (id: string | null, onProgress: (progress: ListingProgress) => void) => Promise<Listing | null>;
  cancel: () => Promise<void>;
  send: (ids: readonly string[]) => Promise<string[]>;
}

const defaultLoaders: ExplorerLoaders = { list: listStorageFolder, cancel: cancelStorageListing, send: sendStorageItemsToReview };

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
 * The Storage page's Browse view: the folder shown, opening another, and sending what you
 * chose to Cleanup's review. Held at the app level so the folder you were in is still there
 * when you come back; nothing is stored.
 */
export function useStorageExplorer(onReviewChanged: () => void, loaders: ExplorerLoaders = defaultLoaders) {
  const [state, setState] = useState<ExplorerState>({ listing: null, loading: false, progress: null, error: null });
  const [sent, setSent] = useState<ReadonlySet<string>>(new Set());
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
        // The review gives its own IDs back, so entries are marked only when all of them went.
        if (done.length === ids.length) {
          setSent((current) => new Set([...current, ...ids]));
        }
        onReviewChanged();
        return done.length === ids.length
          ? `Sent ${plural(done.length, "item")} to review. Nothing has moved.`
          : `Sent ${done.length} of ${ids.length} to review; the rest changed or can’t be chosen. Nothing has moved.`;
      } catch (error) {
        return errorText(error);
      }
    },
    [loaders, onReviewChanged],
  );

  return { state, sent, open, cancel, send };
}

export type StorageExplorer = ReturnType<typeof useStorageExplorer>;
