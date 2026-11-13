import { useRef, useState } from "react";
import { StatusMark } from "../../ui/icons";
import {
  Button,
  FloatingBar,
  IconTile,
  LinkButton,
  Modal,
  PageHeader,
  Pill,
  ProgressBar,
  ScrollEdgeGlass,
  Spinner,
  StatCard,
  Toast,
  useScrolled,
  useToast,
} from "../../ui/components";
import { formatGb, plural, sum } from "../../ui/format";
import { headingIdFor, hrefFor } from "../../shell/routes";
import { actionable, chooseFolderHint, type DeleteJob, type QuarantineBatch, type QuarantineItem, type QuarantineItemStatus, type RestoreOutcome, type RestoreReason } from "./types";
import "./quarantine.css";

interface QuarantineScreenProps {
  batches: readonly QuarantineBatch[];
  restored: ReadonlySet<string>;
  deleted: ReadonlySet<string>;
  job: DeleteJob | null;
  oldest: string;
  /** Why quarantine can't be shown or changed now (its history is unreadable); nothing moved. */
  unavailable?: { text: string; folder: string } | null;
  onRestore: (items: readonly QuarantineItem[]) => Promise<RestoreOutcome>;
  onDelete: (items: readonly QuarantineItem[]) => void;
  onStopDelete: () => void;
  onCloseJob: () => void;
  /** Shows the item in Finder (and with `withOriginal`, what is now at its original place). */
  onRevealInFinder: (item: QuarantineItem, withOriginal: boolean) => Promise<string | null>;
  /** For a `needsFolder` item: asks for its folder in the macOS folder panel. Resolves to a
   * message when it still can't be restored. */
  onChooseFolder: (item: QuarantineItem) => Promise<string | null>;
}

const COLLAPSED_ROWS = 4;

const statusPill: Record<QuarantineItemStatus, { label: string; status: "ok" | "warn" | "muted" }> = {
  restorable: { label: "Restorable", status: "ok" },
  conflict: { label: "Blocked: file exists", status: "warn" },
  folderMissing: { label: "Blocked: folder gone", status: "warn" },
  missing: { label: "Not in quarantine folder", status: "muted" },
  pending: { label: "Not confirmed yet", status: "muted" },
  unverified: { label: "Couldn’t verify", status: "warn" },
  needsFolder: { label: "Choose its folder", status: "warn" },
};

/** Why a pending or unverified item can't be selected. */
const notActionable =
  "Kleari couldn’t confirm this item or the place it came from. It tries again each time it opens, but it may stay unconfirmed. Restore and delete are available once it is confirmed; Show in Finder finds it meanwhile.";

const restoreReasonText: Record<RestoreReason, string> = {
  conflict: "something is already at its original place.",
  folderMissing: "the folder it came from is gone.",
  missing: "it isn’t in the quarantine folder any more.",
  needsAccess: "macOS refused access to its folder.",
  notAllowed: "its recorded place isn’t one Kleari moves items from, so nothing was put there.",
  needsFolder: "choose the folder it came from first.",
  failed: "macOS reported an error.",
};

export function QuarantineScreen(props: QuarantineScreenProps) {
  const { batches, restored, deleted, job } = props;
  const [selection, setSelection] = useState<ReadonlySet<string>>(new Set());
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [dialog, setDialog] = useState<"delete" | "conflict" | null>(null);
  const [conflict, setConflict] = useState<QuarantineItem | null>(null);
  const [acknowledged, setAcknowledged] = useState(false);
  const toast = useToast();
  const [scrolled, onScroll] = useScrolled();
  const listRef = useRef<HTMLDivElement>(null);

  const visible = batches
    .map((batch) => ({ ...batch, items: batch.items.filter((item) => !deleted.has(item.id)) }))
    .filter((batch) => batch.items.length > 0);
  // Pending and unverified items are listed but can't be selected: the backend acts only on
  // confirmed, held items.
  const held = visible.flatMap((batch) => batch.items).filter((item) => !restored.has(item.id) && actionable(item));
  const picked = held.filter((item) => selection.has(item.id));
  const pickedGb = sum(picked, (item) => item.gb);
  const has = picked.length > 0;

  const setSelected = (ids: readonly string[], on: boolean) => {
    const next = new Set(selection);
    for (const id of ids) {
      if (on) {
        next.add(id);
      } else {
        next.delete(id);
      }
    }
    setSelection(next);
  };

  const restoreOne = (item: QuarantineItem) => {
    setSelected([item.id], false);
    void props.onRestore([item]).then((outcome) => {
      if (outcome.kind === "failed") {
        toast.show(outcome.message, "warn");
      } else if (outcome.kind === "unconfirmed") {
        toast.show(`Kleari couldn’t confirm whether ${item.name} was restored. Use Show in Finder to check; Kleari tries again each time it opens.`, "warn");
      } else if (outcome.kind === "blocked") {
        // The dialog is for something in the way, as the backend reported; other reasons are
        // said plainly.
        const reason = outcome.reasons[item.id] ?? "failed";
        if (reason === "conflict") {
          setConflict(item);
          setDialog("conflict");
        } else {
          toast.show(`${item.name} stays in quarantine: ${restoreReasonText[reason]}`, "warn");
        }
      } else {
        toast.show(`Restored ${item.name} to ${item.from}`);
      }
    });
  };

  const restoreSelected = () => {
    setSelection(new Set());
    void props.onRestore(picked).then((outcome) => {
      if (outcome.kind === "failed") {
        toast.show(outcome.message, "warn");
      } else if (outcome.kind === "unconfirmed") {
        toast.show(`Restored ${outcome.restored} · ${plural(outcome.unconfirmed.length, "restore")} couldn’t be confirmed; use Show in Finder to check`, "warn");
      } else if (outcome.kind === "blocked") {
        const folders = Object.values(outcome.reasons).filter((reason) => reason === "needsFolder").length;
        toast.show(
          folders === outcome.blocked.length
            ? `Restored ${outcome.restored} · ${outcome.blocked.length} kept in quarantine until you choose the folders they came from`
            : `Restored ${outcome.restored} · ${outcome.blocked.length} kept in quarantine, because something is in the way, their folder is gone or must be chosen`,
          "warn",
        );
      } else {
        toast.show(`Restored ${plural(outcome.count, "item")} to their original places`);
      }
    });
  };

  const chooseFolder = (item: QuarantineItem) => {
    void props.onChooseFolder(item).then((message) => message && toast.show(message, "warn"));
  };

  const reveal = (item: QuarantineItem, withOriginal: boolean) => {
    void props.onRevealInFinder(item, withOriginal).then((message) => message && toast.show(message, "warn"));
  };

  return (
    <>
      <PageHeader
        title="Quarantine"
        headingId={headingIdFor("quarantine")}
        subtitle="Items here have left their original place but are still on this Mac. Restore any of them at any time."
        actions={<LinkButton href={hrefFor("cleanup")} size="sm">Back to Cleanup</LinkButton>}
      />
      {/* Only the list of batches scrolls; the totals and the note stay put, as on Processes. */}
      <div className="q-body">
        <section aria-label="Quarantine summary" className="v2-grid-3">
          <StatCard size="lg" tone="orange" icon="quarantine" label="Held in quarantine" value={formatGb(sum(held, (item) => item.gb))} sub="Still using space on this Mac" />
          <StatCard size="lg" tone="blue" icon="list" label="Items" value={String(held.length)} sub={`Across ${plural(visible.length, "cleanup")}`} />
          <StatCard size="lg" tone="indigo" icon="clock" label="Oldest item" value={props.oldest} sub="Kept until you decide" />
        </section>

        <div className="v2-info-strip">
          <StatusMark tone="info" />
          <span>Kleari never empties quarantine on its own. Restores never overwrite a file that is already in the original place.</span>
        </div>

        {props.unavailable && (
          <div role="alert" className="v2-banner v2-banner-danger">
            <IconTile tone="red" icon="warning" size={32} />
            <div className="v2-banner-text">
              <strong>Quarantine can’t be shown right now</strong>
              <span>
                {props.unavailable.text} Items already quarantined are untouched in <span className="v2-mono">{props.unavailable.folder}</span>.
              </span>
            </div>
          </div>
        )}

        <div ref={listRef} tabIndex={0} role="region" aria-label="Quarantined items" className="q-list" onScroll={onScroll}>
        <ScrollEdgeGlass visible={scrolled} inset={0} />
        {visible.map((batch) => {
          const open = batch.items.filter((item) => !restored.has(item.id) && actionable(item));
          const allOn = open.length > 0 && open.every((item) => selection.has(item.id));
          const isExpanded = expanded.has(batch.id);
          const rows = isExpanded ? batch.items : batch.items.slice(0, COLLAPSED_ROWS);
          const hidden = batch.items.length - COLLAPSED_ROWS;
          return (
            <section key={batch.id} aria-label={`Quarantined ${batch.title}`} className="v2-card q-batch">
              <div className="q-batch-head">
                <input
                  type="checkbox"
                  aria-label={`Select all from ${batch.title}`}
                  checked={allOn}
                  disabled={open.length === 0}
                  onChange={() => setSelected(open.map((item) => item.id), !allOn)}
                />
                <div className="v2-row-text">
                  <span className="q-batch-title">
                    <h2>{batch.title}</h2>
                    {batch.isNew && <span className="v2-new-badge">NEW</span>}
                  </span>
                  <span className="v2-card-note">{batch.sub}</span>
                </div>
                <span className="q-batch-size">{formatGb(sum(open, (item) => item.gb))}</span>
              </div>
              <ul className="q-rows">
                {rows.map((item) => {
                  const done = restored.has(item.id);
                  const can = !done && actionable(item);
                  const on = can && selection.has(item.id);
                  return (
                    <li key={item.id} className={on ? "selected" : ""} style={{ opacity: done ? 0.55 : 1 }} title={actionable(item) ? undefined : notActionable}>
                      <input type="checkbox" aria-label={`Select ${item.name}`} checked={on} disabled={!can} onChange={() => setSelected([item.id], !on)} />
                      <IconTile tone={item.tone} icon={item.icon} size={30} />
                      <span className="v2-row-text">
                        <span className="v2-row-title">{item.name}</span>
                        <span className="v2-row-sub">From <span className="v2-mono">{item.from}</span></span>
                      </span>
                      <span className="q-size">{formatGb(item.gb)}</span>
                      <span className="q-status">
                        {done ? <Pill status="ok">Restored</Pill> : <Pill status={statusPill[item.status].status}>{statusPill[item.status].label}</Pill>}
                      </span>
                      <span className="q-action">
                        {can && item.status !== "missing" && item.status !== "needsFolder" && (
                          <Button variant="soft" size="sm" aria-label={`Restore ${item.name}`} onClick={() => restoreOne(item)}>Restore</Button>
                        )}
                        {/* Needs its folder to restore, or to settle a restore that was interrupted. */}
                        {!done && item.chooseFolder && (
                          <Button
                            variant="soft"
                            size="sm"
                            aria-label={`Choose the folder ${item.name} came from`}
                            title={`${chooseFolderHint(item)} It is used only to put items back, never scanned.`}
                            onClick={() => chooseFolder(item)}
                          >
                            Choose folder…
                          </Button>
                        )}
                        {!done && !actionable(item) && (
                          <Button variant="soft" size="sm" aria-label={`Show ${item.name} in Finder`} onClick={() => reveal(item, false)}>Show in Finder</Button>
                        )}
                      </span>
                    </li>
                  );
                })}
              </ul>
              {hidden > 0 && (
                <button
                  type="button"
                  className="q-more"
                  aria-expanded={isExpanded}
                  onClick={() => {
                    const next = new Set(expanded);
                    if (isExpanded) {
                      next.delete(batch.id);
                    } else {
                      next.add(batch.id);
                    }
                    setExpanded(next);
                  }}
                >
                  {isExpanded ? "Show fewer" : `Show ${hidden} more`}
                </button>
              )}
            </section>
          );
        })}

        {visible.length === 0 && !props.unavailable && (
          <div className="v2-card v2-card-pad q-empty">
            <IconTile tone="slate" icon="quarantine" size={48} />
            <strong>Quarantine is empty</strong>
            <span className="v2-muted">Items you approve in Cleanup wait here until you restore or delete them.</span>
          </div>
        )}
        </div>
      </div>

      <FloatingBar>
        <div className="v2-float-text">
          <span role="status" className="v2-float-title">{has ? `${picked.length} selected · ${formatGb(pickedGb)}` : "Nothing selected"}</span>
          <span className="v2-float-sub">Restore puts items back where they were. Permanent delete has its own confirmation.</span>
        </div>
        <div className="v2-float-actions">
          <Button variant="dangerOutline" disabled={!has} className="q-delete" onClick={() => { setAcknowledged(false); setDialog("delete"); }}>
            Delete permanently…
          </Button>
          <Button variant="primary" disabled={!has} onClick={restoreSelected}>
            {has ? `Restore ${plural(picked.length, "item")}` : "Restore"}
          </Button>
        </div>
      </FloatingBar>

      <Toast message={toast.message} bottom={92} />

      <Modal open={dialog === "conflict" && conflict !== null} onClose={() => setDialog(null)} labelledBy="q-conflict-title" role="alertdialog">
        {conflict && (
          <div className="v2-dialog-body v2-dialog-centered">
            <IconTile tone="orange" icon="copy" size={56} glow />
            <h2 id="q-conflict-title" className="v2-dialog-title">Can’t restore {conflict.name} yet</h2>
            <p className="v2-dialog-text">
              A new folder has been created at the original place since this was quarantined. Kleari won't overwrite it, so this item stays in quarantine.
            </p>
            <div className="q-conflict-path v2-mono">{conflict.from}/{conflict.name}</div>
            <div className="v2-dialog-actions">
              <Button variant="secondary" onClick={() => setDialog(null)}>Keep in quarantine</Button>
              <Button variant="soft" onClick={() => { setDialog(null); reveal(conflict, true); }}>Show both in Finder</Button>
            </div>
          </div>
        )}
      </Modal>

      <Modal open={dialog === "delete"} onClose={() => setDialog(null)} labelledBy="q-delete-title" role="alertdialog" className="q-delete-dialog">
        <div className="v2-dialog-body">
          <div className="v2-dialog-head">
            <IconTile tone="red" icon="trashLines" size={48} glow />
            <div className="v2-row-text">
              <h2 id="q-delete-title" className="v2-dialog-title">Permanently delete {plural(picked.length, "item")}?</h2>
              <span className="q-danger-text">This can't be undone. These items can't be restored afterwards.</span>
            </div>
          </div>
          <ul className="v2-info-list q-delete-list">
            {picked.map((item) => (
              <li key={item.id}><span className="v2-row-title">{item.name}</span><span className="v2-num v2-muted">{formatGb(item.gb)}</span></li>
            ))}
          </ul>
          <label className={`v2-ack v2-ack-danger${acknowledged ? " on" : ""}`}>
            <input type="checkbox" checked={acknowledged} onChange={() => setAcknowledged(!acknowledged)} />
            <span>I understand these {picked.length} items will be deleted for good.</span>
          </label>
          <div className="v2-dialog-actions q-delete-actions">
            <Button variant="secondary" onClick={() => setDialog(null)}>Cancel</Button>
            <Button
              variant="danger"
              disabled={!acknowledged}
              onClick={() => {
                setDialog(null);
                setSelection(new Set());
                props.onDelete(picked);
              }}
            >
              Delete {formatGb(pickedGb)} permanently
            </Button>
          </div>
        </div>
      </Modal>

      <DeleteProgress job={job} onStop={props.onStopDelete} onClose={props.onCloseJob} />
    </>
  );
}

function DeleteProgress({ job, onStop, onClose }: { job: DeleteJob | null; onStop: () => void; onClose: () => void }) {
  if (job === null) {
    return null;
  }
  const total = job.items.length;
  const totalGb = sum(job.items, (item) => item.gb);
  const finishedItems = job.items.slice(0, job.done);
  const deletedItems = finishedItems.filter((item) => !job.failed.has(item.id));
  const doneGb = sum(deletedItems, (item) => item.gb);
  const left = total - deletedItems.length;
  const partial = job.finished && left > 0;
  const percent = job.finished && !partial ? 100 : Math.round(totalGb ? (doneGb / totalGb) * 100 : 0);
  const current = job.items[job.done];
  const tone = job.finished ? (partial ? "orange" : "gray") : "red";
  const title = job.finished
    ? partial
      ? `${job.stopRequested ? "Stopped" : "Partly done"}: deleted ${deletedItems.length} of ${total}`
      : `Deleted ${plural(total, "item")}`
    : `Deleting ${plural(total, "item")}…`;
  const sub = job.finished
    ? partial
      ? `${plural(left, "item")} still in quarantine${job.failed.size ? `, ${job.failed.size} couldn’t be deleted` : ""}. Nothing else changed.`
      : `${formatGb(doneGb)} freed for good`
    : `Deleting ${current?.name ?? ""} · ${Math.min(job.done + 1, total)} of ${total}`;
  return (
    // Escape can't dismiss a running delete; it finishes the current item or is stopped explicitly.
    <Modal open onClose={() => job.finished && onClose()} labelledBy="q-job-title" role="alertdialog" className="q-delete-dialog">
      <div className="v2-dialog-body q-job">
        <div className="v2-dialog-head">
          <IconTile tone={tone} icon={job.finished ? (partial ? "warning" : "check") : "trashLines"} size={48} glow />
          <div className="v2-row-text">
            <h2 id="q-job-title" className="v2-dialog-title">{title}</h2>
            <span aria-live="polite" className="v2-row-sub q-job-sub">{sub}</span>
          </div>
        </div>
        <div className="q-job-progress">
          <ProgressBar
            percent={job.finished ? percent : Math.max(percent, 4)}
            label="Permanent delete progress"
            valueText={`${formatGb(doneGb)} of ${formatGb(totalGb)} deleted`}
            fill={job.finished ? (partial ? "warn" : "neutral") : "danger"}
            running={!job.finished}
          />
          <div className="q-job-numbers">
            <span>{formatGb(doneGb)} of {formatGb(totalGb)} deleted</span>
            <strong>{percent}%</strong>
          </div>
        </div>
        <ul className="v2-info-list q-job-list">
          {job.items.map((item, index) => {
            const failed = job.failed.has(item.id);
            const isDone = index < job.done && !failed;
            const isNow = index === job.done && !job.finished;
            const state = failed ? "Couldn’t delete, kept" : isDone ? "Deleted" : isNow ? "Deleting…" : job.finished ? "Kept in quarantine" : "Waiting";
            return (
              <li key={item.id}>
                <span className="q-job-icon" aria-hidden="true">
                  {isDone ? <StatusMark tone="muted" /> : isNow ? <Spinner size={12} tone="danger" /> : <span className="q-job-wait" />}
                </span>
                <span className="v2-row-title q-job-name" style={{ opacity: isDone ? 0.6 : 1 }}>{item.name}</span>
                <span className={`q-job-state${isNow ? " now" : failed || (!isDone && job.finished) ? " kept" : ""}`}>{state}</span>
                <span className="q-job-size">{formatGb(item.gb)}</span>
              </li>
            );
          })}
        </ul>
        <div className="q-job-foot">
          <span className="v2-card-note">{job.finished ? "Recorded in Kleari’s history on this Mac" : "Items already deleted can’t come back."}</span>
          {job.finished ? (
            <Button variant="primary" onClick={onClose}>Done</Button>
          ) : (
            <Button variant="secondary" disabled={job.stopRequested} onClick={onStop}>
              {job.stopRequested ? "Stopping after this item…" : "Stop after this item"}
            </Button>
          )}
        </div>
      </div>
    </Modal>
  );
}
