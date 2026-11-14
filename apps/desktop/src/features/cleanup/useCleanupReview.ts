import { useCallback, useEffect, useRef, useState } from "react";
import type { ApproveOutcome, CleanupPreview, DeleteProgress, DeleteSummary, MoveFailure, MoveProgress, ReviewFinding, ReviewList } from "../../contracts/cleanup";
import {
  addStorageFileToReview,
  approveCleanupPreview,
  createCleanupPreview,
  deleteQuarantineItems,
  getCleanupReview,
  revealReviewItem,
  sendDeveloperItemsToReview,
} from "../../lib/cleanupIpc";
import type { PreviewItemState, PreviewStage } from "../../v2/screens/cleanup/PreviewScreen";
import { plural } from "../../v2/ui/format";
import { storageErrorText } from "../storage/format";
import { errorKind } from "./useCleanupScan";

export interface ReviewLoaders {
  review: () => Promise<ReviewList>;
  sendDeveloper: (ids: readonly string[]) => Promise<string[]>;
  addStorageFile: (reviewId: string) => Promise<boolean>;
  reveal: (id: string) => Promise<void>;
  createPreview: (ids: readonly string[]) => Promise<CleanupPreview>;
  approve: (previewId: string, onProgress: (progress: MoveProgress) => void) => Promise<ApproveOutcome>;
  /** Quarantine's permanent delete, for "Delete permanently": what an approval just moved. */
  remove: (heldIds: readonly string[], onProgress: (progress: DeleteProgress) => void) => Promise<DeleteSummary>;
}

const defaultLoaders: ReviewLoaders = {
  review: getCleanupReview,
  sendDeveloper: sendDeveloperItemsToReview,
  addStorageFile: addStorageFileToReview,
  reveal: revealReviewItem,
  createPreview: createCleanupPreview,
  approve: approveCleanupPreview,
  remove: deleteQuarantineItems,
};

/** Quarantine (restorable), or delete permanently straight after the move into quarantine. */
export type ApproveMode = "quarantine" | "delete";

export type PreviewStatus = PreviewStage | "idle" | "preparing" | "error";

export interface PreviewState {
  status: PreviewStatus;
  preview: CleanupPreview | null;
  /** The selection the preview was made from, in order, as the review showed it then (moved
   * items leave the review, but the result still lists them). */
  items: readonly ReviewFinding[];
  states: Readonly<Record<string, PreviewItemState>>;
  failures: Readonly<Record<string, MoveFailure>>;
  /** Items that changed after the preview, which blocked approval. */
  changed: readonly string[];
  finishedAtMs: number | null;
  error: string | null;
  /** What the approval was for. */
  mode: ApproveMode;
  /** Why the permanent delete after the move didn't run; the items stay in Quarantine. */
  deleteError: string | null;
}

const idlePreview: PreviewState = { status: "idle", preview: null, items: [], states: {}, failures: {}, changed: [], finishedAtMs: null, error: null, mode: "quarantine", deleteError: null };

export const historyText = "Kleari’s history can’t be written, so nothing was moved. Every move needs a record of where the item came from.";

/**
 * The review (Cleanup step 2), the selection, items sent from Storage and Developer, and the
 * preview and approval (step 3). Held at the app level so the selection and a running
 * approval survive moving between the steps.
 */
export function useCleanupReview(loaders: ReviewLoaders = defaultLoaders) {
  const [review, setReview] = useState<ReviewList | null>(null);
  const [reviewError, setReviewError] = useState<string | null>(null);
  const [selection, setSelectionState] = useState<ReadonlySet<string>>(new Set());
  const [developerSent, setDeveloperSent] = useState<ReadonlySet<string>>(new Set());
  const [storageSent, setStorageSent] = useState<ReadonlySet<string>>(new Set());
  const [preview, setPreview] = useState<PreviewState>(idlePreview);
  const mounted = useRef(true);
  const approving = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const refresh = useCallback(() => {
    loaders.review().then(
      (next) => {
        if (!mounted.current) {
          return;
        }
        setReview(next);
        setReviewError(null);
        // Only items still in the review and still eligible stay selected.
        const eligible = new Set(next.findings.filter((finding) => finding.eligibility === "eligible").map((finding) => finding.id));
        setSelectionState((current) => new Set([...current].filter((id) => eligible.has(id))));
      },
      (error: unknown) => {
        if (mounted.current) {
          setReviewError(storageErrorText[errorKind(error)]);
        }
      },
    );
  }, [loaders]);

  const setSelection = useCallback((next: ReadonlySet<string>) => {
    setSelectionState(next);
    // A new selection needs a new preview.
    if (!approving.current) {
      setPreview(idlePreview);
    }
  }, []);

  /** Sends Developer items (by their review IDs) to the review. Resolves to a message. */
  const sendDeveloper = useCallback(
    async (ids: readonly string[]): Promise<string> => {
      try {
        const sent = await loaders.sendDeveloper(ids);
        setDeveloperSent((current) => new Set([...current, ...sent]));
        refresh();
        return sent.length === ids.length
          ? `Sent ${plural(sent.length, "item")} to review. Nothing has moved.`
          : `Sent ${sent.length} of ${ids.length} to review. Scan again to send the rest. Nothing has moved.`;
      } catch (error) {
        return storageErrorText[errorKind(error)];
      }
    },
    [loaders, refresh],
  );

  const addStorageFile = useCallback(
    async (reviewId: string, name: string): Promise<string> => {
      try {
        const added = await loaders.addStorageFile(reviewId);
        if (!added) {
          return "That file is no longer in the last analysis. Analyze again.";
        }
        setStorageSent((current) => new Set([...current, reviewId]));
        refresh();
        return `Added ${name} to Cleanup review. Nothing has moved.`;
      } catch (error) {
        return storageErrorText[errorKind(error)];
      }
    },
    [loaders, refresh],
  );

  /** Resolves to a message when Finder couldn't show it. */
  const reveal = useCallback(
    async (id: string): Promise<string | null> => {
      try {
        await loaders.reveal(id);
        return null;
      } catch (error) {
        return storageErrorText[errorKind(error)];
      }
    },
    [loaders],
  );

  /** Checks the selected items again and binds them into a preview. */
  const createPreview = useCallback(
    (items: readonly ReviewFinding[]) => {
      if (approving.current) {
        return;
      }
      setPreview({ ...idlePreview, status: "preparing", items });
      loaders.createPreview(items.map((item) => item.id)).then(
        (made) => {
          if (!mounted.current) {
            return;
          }
          const states: Record<string, PreviewItemState> = {};
          // The preview's sizes are measured now, so they replace the scan's.
          const measured = items.map((item) => ({ ...item, bytes: made.items.find((planned) => planned.id === item.id)?.bytes ?? item.bytes }));
          for (const item of made.items) {
            states[item.id] = item.state === "planned" ? "planned" : "excluded";
          }
          setPreview({ ...idlePreview, status: "review", preview: made, items: measured, states });
          // Items that changed since the scan are now marked so in the review.
          refresh();
        },
        (error: unknown) => {
          if (mounted.current) {
            setPreview({ ...idlePreview, status: "error", items, error: storageErrorText[errorKind(error)] });
          }
        },
      );
    },
    [loaders, refresh],
  );

  /**
   * Deletes, through Quarantine's permanent delete, exactly what an approval just moved
   * (`held`: quarantine item → finding). Whatever isn't deleted stays in Quarantine.
   */
  const deleteMoved = useCallback(
    (held: ReadonlyMap<string, string>, finishedAtMs: number) => {
      const done = (patch: Partial<PreviewState>) => {
        approving.current = false;
        if (mounted.current) {
          setPreview((state) => ({ ...state, status: "result", finishedAtMs, ...patch }));
        }
      };
      if (held.size === 0) {
        done({});
        return;
      }
      setPreview((state) => {
        const states: Record<string, PreviewItemState> = { ...state.states };
        for (const id of held.values()) {
          states[id] = "deleting";
        }
        return { ...state, status: "deleting", states };
      });
      loaders
        .remove([...held.keys()], (progress) => {
          const id = held.get(progress.id);
          if (mounted.current && id !== undefined && progress.state !== "deleting") {
            setPreview((state) => ({ ...state, states: { ...state.states, [id]: progress.state === "deleted" ? "deleted" : "notDeleted" } }));
          }
        })
        .then(
          (summary) => {
            if (!mounted.current) {
              approving.current = false;
              return;
            }
            setPreview((state) => {
              const states: Record<string, PreviewItemState> = { ...state.states };
              for (const heldId of summary.deleted) {
                const id = held.get(heldId);
                if (id !== undefined) {
                  states[id] = "deleted";
                }
              }
              for (const heldId of [...summary.failed, ...summary.kept]) {
                const id = held.get(heldId);
                if (id !== undefined) {
                  states[id] = "notDeleted";
                }
              }
              return { ...state, states };
            });
            done({});
          },
          (error: unknown) => {
            if (mounted.current) {
              setPreview((state) => {
                const states: Record<string, PreviewItemState> = { ...state.states };
                for (const id of held.values()) {
                  if (states[id] === "deleting") {
                    states[id] = "notDeleted";
                  }
                }
                return { ...state, states };
              });
            }
            done({ deleteError: storageErrorText[errorKind(error)] });
          },
        );
    },
    [loaders],
  );

  const approve = useCallback((mode: ApproveMode = "quarantine") => {
    const current = preview.preview;
    if (!current || approving.current) {
      return;
    }
    approving.current = true;
    const planned = current.items.filter((item) => item.state === "planned").map((item) => item.id);
    const waiting: Record<string, PreviewItemState> = { ...preview.states };
    for (const id of planned) {
      waiting[id] = "waiting";
    }
    setPreview((state) => ({ ...state, status: "running", states: waiting, mode, deleteError: null }));
    loaders
      .approve(current.id, (progress) => {
        if (!mounted.current) {
          return;
        }
        setPreview((state) => {
          const states: Record<string, PreviewItemState> = { ...state.states, [progress.id]: progress.state };
          const failures = progress.state === "failed" ? { ...state.failures, [progress.id]: progress.reason } : state.failures;
          return { ...state, states, failures };
        });
      })
      .then(
        (outcome) => {
          // A delete after the move keeps the approval going until it finishes.
          const deleting = mode === "delete" && outcome.status === "finished";
          if (!deleting) {
            approving.current = false;
          }
          if (!mounted.current) {
            approving.current = false;
            return;
          }
          if (outcome.status === "finished") {
            const states: Record<string, PreviewItemState> = { ...preview.states };
            const failures: Record<string, MoveFailure> = {};
            const held = new Map<string, string>();
            for (const item of outcome.items) {
              states[item.id] = item.state === "checking" ? "failed" : item.state;
              if (item.state === "failed") {
                failures[item.id] = item.reason;
              }
              if (item.state === "moved" && item.heldId !== undefined) {
                held.set(item.heldId, item.id);
              }
            }
            setPreview((state) => ({ ...state, status: deleting ? "deleting" : "result", states, failures, finishedAtMs: outcome.finishedAtMs }));
            setSelectionState(new Set());
            refresh();
            if (deleting) {
              deleteMoved(held, outcome.finishedAtMs);
            }
          } else if (outcome.status === "stale" || outcome.status === "noPreview") {
            const changed = outcome.status === "stale" ? outcome.changed : [];
            const states: Record<string, PreviewItemState> = { ...preview.states };
            for (const id of changed) {
              states[id] = "changed";
            }
            setPreview((state) => ({ ...state, status: "stale", states, changed }));
          } else {
            setPreview((state) => ({ ...state, status: "error", states: preview.states, error: historyText }));
          }
        },
        (error: unknown) => {
          approving.current = false;
          if (mounted.current) {
            setPreview((state) => ({ ...state, status: "error", states: preview.states, error: storageErrorText[errorKind(error)] }));
          }
        },
      );
  }, [loaders, preview, refresh, deleteMoved]);

  /** Makes a new preview of the same selection after one went out of date. */
  const rebuild = useCallback(() => {
    createPreview(preview.items);
  }, [createPreview, preview.items]);

  /** Back to an empty preview, such as after Done. */
  const clearPreview = useCallback(() => {
    if (!approving.current) {
      setPreview(idlePreview);
    }
  }, []);

  return {
    review,
    reviewError,
    refresh,
    selection,
    setSelection,
    developerSent,
    storageSent,
    sendDeveloper,
    addStorageFile,
    reveal,
    preview,
    createPreview,
    approve,
    rebuild,
    clearPreview,
  };
}
