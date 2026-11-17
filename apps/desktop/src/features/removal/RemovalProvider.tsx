import { createContext, useCallback, useContext, useRef, useState, type ReactNode } from "react";
import type { CleanupPreview, MoveFailure, PlannedItem } from "../../contracts/cleanup";
import { approveCleanupPreview, deleteCleanupPreview, restoreQuarantineItems, revealReviewItem } from "../../lib/cleanupIpc";
import { storageErrorText } from "../storage/format";
import { errorKind } from "../cleanup/useCleanupScan";
import { whyNotText } from "../cleanup/cleanupView";
import { Button, Modal } from "../../v2/ui/components";
import { Glyph } from "../../v2/ui/icons";
import { formatGb, plural, sum } from "../../v2/ui/format";
import "./removal.css";

/** What a page asks to remove where it shows it. */
export interface RemovalRequest {
  /** What is being removed, for the sheet's title, such as "3 files". */
  what: string;
  /** Prepares the items (Rust checks each again now) and returns their preview. */
  prepare: () => Promise<CleanupPreview>;
  /** After anything was deleted, moved or put back, so the page shows what is left: how many
   * are gone from where they were now (0 after Undo). */
  onDone?: (gone: number) => void;
}

/** Delete for good where they are, or move into quarantine to keep the chance to put back. */
type Mode = "delete" | "quarantine";

interface Result {
  mode: Mode;
  /** How many were deleted or moved, and their size. */
  count: number;
  bytes: number;
  /** For a move: the quarantine items, so Undo can put them back. */
  held: string[];
  /** What didn't go, and why, in words. */
  problems: string[];
  undone: boolean;
}

type Phase =
  | { kind: "closed" }
  | { kind: "preparing"; request: RemovalRequest }
  | { kind: "ready"; request: RemovalRequest; preview: CleanupPreview }
  | { kind: "working"; request: RemovalRequest; preview: CleanupPreview; mode: Mode }
  | { kind: "finished"; request: RemovalRequest; result: Result }
  | { kind: "failed"; request: RemovalRequest; message: string };

const failureText: Record<MoveFailure, string> = {
  needsAccess: "macOS didn't give Kleari access to it",
  missing: "it's no longer there",
  changed: "it changed since it was checked",
  otherVolume: "part of it is on another disk",
  exists: "something with that name is already in quarantine",
  folderUnavailable: "its folder couldn't be opened",
  quarantineUnavailable: "quarantine couldn't be reached",
  failed: "macOS didn't remove it",
};

const staleText = "Something changed since these were checked, so nothing was removed. Try again.";

const RemovalContext = createContext<(request: RemovalRequest) => void>(() => undefined);

/** Opens the confirm sheet for a page's items: `remove({ what, prepare, onDone })`. */
export function useRemoval(): (request: RemovalRequest) => void {
  return useContext(RemovalContext);
}

const errorText = (error: unknown) => storageErrorText[errorKind(error)];

/**
 * Removing what a page shows, on that page: one sheet for every page. Items are checked again
 * when it opens and once more right before each goes. Delete permanently removes them where
 * they are; Move to quarantine keeps them so they can be put back.
 */
export function RemovalProvider({ children }: { children: ReactNode }) {
  const [phase, setPhase] = useState<Phase>({ kind: "closed" });
  // The request a late answer belongs to: an answer for a closed or replaced sheet is dropped.
  const current = useRef<RemovalRequest | null>(null);
  const show = (request: RemovalRequest, next: Phase) => {
    if (current.current === request) {
      setPhase(next);
    }
  };

  const remove = useCallback((request: RemovalRequest) => {
    current.current = request;
    setPhase({ kind: "preparing", request });
    request.prepare().then(
      (preview) => current.current === request && setPhase({ kind: "ready", request, preview }),
      (error: unknown) => current.current === request && setPhase({ kind: "failed", request, message: errorText(error) }),
    );
  }, []);

  const close = useCallback(() => {
    current.current = null;
    setPhase({ kind: "closed" });
  }, []);

  const confirm = (mode: Mode) => {
    if (phase.kind !== "ready") {
      return;
    }
    const { request, preview } = phase;
    const named = (id: string) => preview.items.find((item) => item.id === id);
    setPhase({ kind: "working", request, preview, mode });
    const finish = (result: Result) => {
      request.onDone?.(result.count);
      show(request, { kind: "finished", request, result });
    };
    const failed = (error: unknown) => show(request, { kind: "failed", request, message: errorText(error) });
    if (mode === "delete") {
      deleteCleanupPreview(preview.id, () => undefined).then((outcome) => {
        if (outcome.status !== "finished") {
          show(request, { kind: "failed", request, message: staleText });
          return;
        }
        const gone = outcome.items.filter((item) => item.state === "deleted");
        const problems = outcome.items.flatMap((item) => (item.state === "failed" ? [`${named(item.id)?.name ?? "An item"}: ${failureText[item.reason]}.`] : []));
        finish({ mode, count: gone.length, bytes: sum(gone, (item) => named(item.id)?.bytes ?? 0), held: [], problems, undone: false });
      }, failed);
      return;
    }
    approveCleanupPreview(preview.id, () => undefined).then((outcome) => {
      if (outcome.status !== "finished") {
        const message = outcome.status === "historyUnavailable" ? "Kleari's history couldn't be read, so nothing moved." : staleText;
        show(request, { kind: "failed", request, message });
        return;
      }
      const moved = outcome.items.filter((item) => item.state === "moved");
      const held = moved.flatMap((item) => (item.state === "moved" && item.heldId !== undefined ? [item.heldId] : []));
      const problems = outcome.items.flatMap((item) => (item.state === "failed" ? [`${named(item.id)?.name ?? "An item"}: ${failureText[item.reason]}.`] : []));
      finish({ mode, count: moved.length, bytes: sum(moved, (item) => named(item.id)?.bytes ?? 0), held, problems, undone: false });
    }, failed);
  };

  const undo = () => {
    if (phase.kind !== "finished" || phase.result.mode !== "quarantine" || phase.result.held.length === 0) {
      return;
    }
    const { request, result } = phase;
    restoreQuarantineItems(result.held).then(
      (results) => {
        request.onDone?.(0);
        const stayed = results.filter((entry) => entry.outcome !== "restored").length;
        const problems = stayed === 0 ? [] : [`${plural(stayed, "item")} couldn't go back and ${stayed === 1 ? "is" : "are"} still in quarantine.`];
        show(request, { kind: "finished", request, result: { ...result, undone: true, problems } });
      },
      (error: unknown) => show(request, { kind: "failed", request, message: errorText(error) }),
    );
  };

  const busy = phase.kind === "preparing" || phase.kind === "working";

  return (
    <RemovalContext.Provider value={remove}>
      {children}
      <Modal open={phase.kind !== "closed"} onClose={() => !busy && close()} labelledBy="removal-title" role="alertdialog" className="removal-sheet">
        {phase.kind !== "closed" && (
          <div className="v2-dialog-body removal-body">
            <Sheet phase={phase} onConfirm={confirm} onClose={close} onUndo={undo} />
          </div>
        )}
      </Modal>
    </RemovalContext.Provider>
  );
}

function ItemRow({ item }: { item: PlannedItem }) {
  const removable = item.state === "planned";
  const why = item.state === "changed" ? "it changed since it was found; look again to remove it" : item.whyNot ? whyNotText[item.whyNot] : "Kleari can't remove it now";
  return (
    <li className={`removal-item${removable ? "" : " held"}`}>
      <span className="v2-row-text">
        <span className="removal-item-name">{item.name}</span>
        <span className="removal-item-sub">{removable ? item.location : `Stays: ${why}`}</span>
      </span>
      <span className="removal-item-size">{formatGb(item.bytes / 1e9)}</span>
      <button type="button" className="removal-reveal" aria-label={`Show ${item.name} in Finder`} title="Show in Finder" onClick={() => void revealReviewItem(item.id).catch(() => undefined)}>
        <Glyph name="folder" size={14} strokeWidth={2} />
      </button>
    </li>
  );
}

function Sheet({ phase, onConfirm, onClose, onUndo }: { phase: Exclude<Phase, { kind: "closed" }>; onConfirm: (mode: Mode) => void; onClose: () => void; onUndo: () => void }) {
  if (phase.kind === "preparing") {
    return (
      <>
        <h2 id="removal-title" className="v2-dialog-title">Checking {phase.request.what}…</h2>
        <p className="v2-dialog-text">Making sure each one is still there and safe to remove.</p>
      </>
    );
  }
  if (phase.kind === "failed") {
    return (
      <>
        <h2 id="removal-title" className="v2-dialog-title">Nothing was removed</h2>
        <p className="v2-dialog-text">{phase.message}</p>
        <div className="v2-dialog-actions">
          <Button variant="primary" onClick={onClose}>OK</Button>
        </div>
      </>
    );
  }
  if (phase.kind === "finished") {
    const { result } = phase;
    const size = formatGb(result.bytes / 1e9);
    const title = result.undone
      ? "Put back"
      : result.count === 0
        ? "Nothing was removed"
        : result.mode === "delete"
          ? `Deleted ${plural(result.count, "item")}`
          : `Moved ${plural(result.count, "item")} to quarantine`;
    const text = result.undone
      ? "They're back where they were."
      : result.count === 0
        ? "None of them could be removed."
        : result.mode === "delete"
          ? `${size} freed.`
          : `${size} freed. ${result.count === 1 ? "It stays" : "They stay"} in quarantine until you delete ${result.count === 1 ? "it" : "them"} there, so you can put ${result.count === 1 ? "it" : "them"} back any time.`;
    return (
      <>
        <h2 id="removal-title" className="v2-dialog-title">{title}</h2>
        <p className="v2-dialog-text">{text}</p>
        {result.problems.length > 0 && (
          <ul className="removal-problems">
            {result.problems.map((problem) => (
              <li key={problem}>{problem}</li>
            ))}
          </ul>
        )}
        <div className="v2-dialog-actions">
          {result.mode === "quarantine" && !result.undone && result.held.length > 0 && (
            <Button variant="secondary" onClick={onUndo}>Undo</Button>
          )}
          <Button variant="primary" onClick={onClose}>Done</Button>
        </div>
      </>
    );
  }
  const planned = phase.preview.items.filter((item) => item.state === "planned");
  const held = phase.preview.items.filter((item) => item.state !== "planned");
  const size = formatGb(sum(planned, (item) => item.bytes) / 1e9);
  const working = phase.kind === "working" ? phase.mode : null;
  return (
    <>
      <h2 id="removal-title" className="v2-dialog-title">
        {planned.length === 0 ? `None of ${phase.request.what} can be removed now` : `Remove ${phase.request.what}?`}
      </h2>
      {planned.length > 0 && (
        <p className="v2-dialog-text">
          {plural(planned.length, "item")}, {size}. Delete them for good, or move them to quarantine if you might want them back.
        </p>
      )}
      <ul className="removal-list" aria-label="What will be removed">
        {[...planned, ...held].map((item) => (
          <ItemRow key={item.id} item={item} />
        ))}
      </ul>
      <div className="v2-dialog-actions">
        <Button variant="secondary" disabled={working !== null} onClick={onClose}>Cancel</Button>
        {planned.length > 0 && (
          <>
            <Button variant="soft" disabled={working !== null} onClick={() => onConfirm("quarantine")}>
              {working === "quarantine" ? "Moving…" : "Move to quarantine"}
            </Button>
            <Button variant="danger" disabled={working !== null} onClick={() => onConfirm("delete")}>
              {working === "delete" ? "Deleting…" : `Delete ${size} permanently`}
            </Button>
          </>
        )}
      </div>
    </>
  );
}
