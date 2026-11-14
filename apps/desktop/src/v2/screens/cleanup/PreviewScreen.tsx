import { useState } from "react";
import { Glyph, StatusMark, type IconName } from "../../ui/icons";
import { Button, Dot, IconTile, LinkButton, Modal, PageHeader, ScrollBody, Spinner, type Tone } from "../../ui/components";
import { formatGb, plural, sum } from "../../ui/format";
import { headingIdFor, hrefFor } from "../../shell/routes";
import { CleanupSteps } from "./CleanupSteps";
import { categoryMeta } from "./findingMeta";
import type { Finding, FindingCategory } from "./types";
import "./preview.css";

/** `deleting`: moved into quarantine and now being deleted permanently ("Delete permanently"). */
export type PreviewStage = "review" | "stale" | "running" | "deleting" | "result";
/** `excluded`: left out when the preview was made (it changed since the scan, or isn't eligible now). */
/** `unconfirmed`: moved, but the move couldn't be checked; it is listed in Quarantine. */
/** `deleted` / `notDeleted`: after "Delete permanently"; one not deleted stays in Quarantine. */
export type PreviewItemState =
  | "planned"
  | "changed"
  | "checking"
  | "waiting"
  | "moved"
  | "unconfirmed"
  | "failed"
  | "excluded"
  | "deleting"
  | "deleted"
  | "notDeleted";

/** Quarantine (restorable), or delete permanently straight after the move. */
export type PreviewMode = "quarantine" | "delete";

interface PreviewScreenProps {
  items: readonly Finding[];
  /** `preparing` while each item is checked; `error` when the preview or approval failed. */
  stage: PreviewStage | "preparing" | "error";
  states: Record<string, PreviewItemState>;
  preparedAt: string;
  scanAt: string;
  finishedAt: string;
  quarantineFolder: string;
  staleItem?: Finding;
  failedItem?: Finding;
  /** Why the failed item stayed where it was. */
  failedText?: string;
  /** Why the preview or approval couldn't go on; nothing moved. */
  errorText?: string | null;
  /** What each moved category means afterwards, e.g. Xcode rebuilding DerivedData. */
  consequences: string;
  /** What the approval was (or is) for. */
  mode: PreviewMode;
  /** Why the permanent delete didn't run after the move; the items stay in Quarantine. */
  deleteError?: string | null;
  onApprove: (mode: PreviewMode) => void;
  onRebuild: () => void;
}

const effects: Record<PreviewItemState, { text: string; icon: IconName | "spin"; className: string }> = {
  planned: { text: "Move to quarantine", icon: "arrowRight", className: "pv-effect-planned" },
  changed: { text: "Changed after preview", icon: "warning", className: "pv-effect-danger" },
  checking: { text: "Checking…", icon: "spin", className: "pv-effect-muted" },
  waiting: { text: "Waiting", icon: "arrowRight", className: "pv-effect-muted" },
  moved: { text: "Quarantined, restorable", icon: "check", className: "pv-effect-ok" },
  unconfirmed: { text: "Moved, not confirmed", icon: "info", className: "pv-effect-muted" },
  failed: { text: "Not moved, left in place", icon: "warning", className: "pv-effect-warn" },
  excluded: { text: "Left out, changed or not eligible now", icon: "blocked", className: "pv-effect-muted" },
  deleting: { text: "Deleting…", icon: "spin", className: "pv-effect-muted" },
  deleted: { text: "Deleted permanently", icon: "check", className: "pv-effect-ok" },
  notDeleted: { text: "Not deleted, still in Quarantine", icon: "warning", className: "pv-effect-warn" },
};

const steps: readonly { title: string; sub: string; tone: Tone }[] = [
  { title: "Kleari checks each item again", sub: "Same file, same size, still allowed, access still granted. If anything changed, nothing moves and you review again.", tone: "blue" },
  { title: "Items move to quarantine", sub: "They leave their current place but stay on this Mac, in Kleari’s quarantine folder.", tone: "violet" },
  { title: "The result is saved on this Mac", sub: "What moved, anything left in place and why, so each item can be restored from Quarantine.", tone: "green" },
];

function SummaryRing({ fraction, tone, heroTone, heroIcon }: { fraction: number; tone: Tone; heroTone: Tone; heroIcon: IconName }) {
  const circumference = 2 * Math.PI * 36;
  return (
    <div className={`pv-ring tone-${tone}`}>
      <svg aria-hidden="true" width="84" height="84" viewBox="0 0 84 84">
        <defs>
          <linearGradient id="pv-ring" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" className="v2-stop-1" />
            <stop offset="1" className="v2-stop-2" />
          </linearGradient>
        </defs>
        <circle cx="42" cy="42" r="36" fill="none" className="v2-track-stroke" strokeWidth="7" />
        <circle
          cx="42"
          cy="42"
          r="36"
          fill="none"
          stroke="url(#pv-ring)"
          strokeWidth="7"
          strokeLinecap="round"
          strokeDasharray={`${(circumference * fraction).toFixed(1)} ${circumference.toFixed(1)}`}
          transform="rotate(-90 42 42)"
        />
      </svg>
      <span className={`pv-ring-hero tone-${heroTone}`}><Glyph name={heroIcon} size={24} strokeWidth={2.1} /></span>
    </div>
  );
}

export function PreviewScreen(props: PreviewScreenProps) {
  const { items: all, stage, states, staleItem, failedItem, mode } = props;
  const [acknowledged, setAcknowledged] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleteAcknowledged, setDeleteAcknowledged] = useState(false);
  const result = stage === "result";
  const deleteMode = mode === "delete";
  const excluded = all.filter((item) => states[item.id] === "excluded");
  // Items left out when the preview was made are listed but never move.
  const items = all.filter((item) => states[item.id] !== "excluded");
  // An unconfirmed move did happen, so it counts as moved, never as left in place; so does an
  // item deleted afterwards, or one whose delete didn't go ahead (it is in Quarantine).
  const movedStates: ReadonlySet<PreviewItemState | undefined> = new Set(["moved", "unconfirmed", "deleting", "deleted", "notDeleted"]);
  const movedItems = items.filter((item) => movedStates.has(states[item.id]));
  const deletedItems = items.filter((item) => states[item.id] === "deleted");
  const keptItems = items.filter((item) => states[item.id] === "notDeleted" || (deleteMode && states[item.id] === "moved"));
  const unconfirmed = items.filter((item) => states[item.id] === "unconfirmed");
  const totalGb = sum(items, (item) => item.gb);
  const movedGb = sum(movedItems, (item) => item.gb);
  const deletedGb = sum(deletedItems, (item) => item.gb);
  const partial = result && movedItems.length < items.length;

  const byCategory = new Map<FindingCategory, number>();
  for (const item of items) {
    byCategory.set(item.category, (byCategory.get(item.category) ?? 0) + item.gb);
  }
  const categories = [...byCategory.entries()].sort((a, b) => b[1] - a[1]);
  const categoryText = categories.map(([key, gb]) => `${categoryMeta[key].label} ${formatGb(gb)}`).join(", ");

  if (stage === "preparing") {
    return (
      <>
        <PageHeader above={<CleanupSteps current={2} />} title="Preview and approve" headingId={headingIdFor("preview")} subtitle="Checking each selected item again." />
        <div className="v2-scroll">
          <div className="v2-card v2-card-pad pv-empty">
            <Spinner size={20} />
            <strong role="status">Making the preview…</strong>
            <span className="v2-muted">Kleari reads each item again: same item, its size now, still allowed. Nothing moves yet.</span>
          </div>
        </div>
      </>
    );
  }

  if (all.length === 0 || (items.length === 0 && stage !== "error")) {
    return (
      <>
        <PageHeader above={<CleanupSteps current={2} />} title="Preview and approve" headingId={headingIdFor("preview")} subtitle="Nothing is selected yet." />
        <div className="v2-scroll">
          <div className="v2-card v2-card-pad pv-empty">
            <IconTile tone="slate" icon="quarantine" size={48} />
            <strong>{all.length === 0 ? "Pick items in Review results first" : "Nothing in this selection can move now"}</strong>
            <span className="v2-muted">
              {all.length === 0
                ? "A preview lists exactly what you selected. Nothing moves until you approve it here."
                : "Each selected item changed since the scan or isn’t eligible right now. Scan again to review them."}
            </span>
            <LinkButton href={hrefFor("review")} variant="primary">Back to results</LinkButton>
          </div>
        </div>
      </>
    );
  }

  const deletedResult = result && deleteMode;
  const doneCount = deletedResult ? deletedItems.length : movedItems.length;
  const partlyDone = result && doneCount < items.length;
  const ringTone: Tone = result ? (partlyDone ? "orange" : "green") : deleteMode && stage !== "review" ? "red" : "blue";
  const heroTone: Tone = result ? "green" : stage === "stale" ? "red" : deleteMode && stage !== "review" ? "red" : "blue";
  const heroIcon: IconName = result ? "check" : stage === "stale" ? "warning" : deleteMode && stage !== "review" ? "trashLines" : "quarantine";
  const fraction = result ? doneCount / Math.max(items.length, 1) : 1;
  const failedCount = items.length - movedItems.length;

  return (
    <>
      <PageHeader
        above={<CleanupSteps current={2} />}
        title={deletedResult ? "Delete finished" : result ? "Quarantine finished" : "Preview and approve"}
        headingId={headingIdFor("preview")}
        subtitle={
          result
            ? `Today ${props.finishedAt} · Saved in Activity`
            : `Preview made at ${props.preparedAt} from the scan at ${props.scanAt} · valid while these items stay unchanged`
        }
      />
      <ScrollBody label="Preview" className="pv-body">
        {stage === "error" && props.errorText && (
          <div role="alert" className="v2-banner v2-banner-danger">
            <IconTile tone="red" icon="warning" size={32} />
            <div className="v2-banner-text">
              <strong className="pv-danger-text">Nothing was moved</strong>
              <span>{props.errorText}</span>
            </div>
            <Button variant="primary" className="pv-banner-btn" onClick={props.onRebuild}>Try again</Button>
          </div>
        )}
        {stage === "stale" && (
          <div role="alert" className="v2-banner v2-banner-danger">
            <IconTile tone="red" icon="warning" size={32} />
            <div className="v2-banner-text">
              <strong className="pv-danger-text">This preview is out of date, so approval is blocked</strong>
              <span>{staleItem ? `${staleItem.name} changed after this preview was made.` : "The review changed after this preview was made."} Nothing has been moved.</span>
            </div>
            <Button variant="primary" className="pv-banner-btn" onClick={props.onRebuild}>Rebuild preview</Button>
          </div>
        )}
        {excluded.length > 0 && !result && (
          <div role="status" className="v2-banner v2-banner-warn">
            <IconTile tone="orange" icon="blocked" size={32} />
            <div className="v2-banner-text">
              <strong>{plural(excluded.length, "item")} left out of this preview</strong>
              <span>{excluded.map((item) => item.name).join(", ")} changed since the scan or can’t move right now. Scan again to review them.</span>
            </div>
          </div>
        )}
        {result && unconfirmed.length > 0 && (
          <div role="status" className="v2-banner v2-banner-warn">
            <IconTile tone="orange" icon="info" size={32} />
            <div className="v2-banner-text">
              <strong>{plural(unconfirmed.length, "item")} moved but not yet confirmed</strong>
              <span>
                {unconfirmed.map((item) => item.name).join(", ")} moved into quarantine, but Kleari couldn’t check {unconfirmed.length === 1 ? "it" : "them"} afterwards. {unconfirmed.length === 1 ? "It is" : "They are"} listed in Quarantine, where Show in Finder finds {unconfirmed.length === 1 ? "it" : "them"}. Kleari tries to confirm {unconfirmed.length === 1 ? "it" : "them"} each time it opens; until then {unconfirmed.length === 1 ? "it" : "they"} can’t be restored or deleted from Kleari.
              </span>
            </div>
          </div>
        )}
        {result && deleteMode && (props.deleteError || keptItems.length > 0) && (
          <div role="status" className="v2-banner v2-banner-warn">
            <IconTile tone="orange" icon="warning" size={32} />
            <div className="v2-banner-text">
              <strong>{plural(keptItems.length, "item")} moved to quarantine but not deleted</strong>
              <span>
                {props.deleteError ?? "Kleari couldn’t delete them."} {keptItems.length === 1 ? "It is" : "They are"} still in Quarantine, where you can restore or delete {keptItems.length === 1 ? "it" : "them"}.
              </span>
            </div>
          </div>
        )}
        {partial && failedItem && (
          <div role="status" className="v2-banner v2-banner-warn">
            <IconTile tone="orange" icon="warning" size={32} />
            <div className="v2-banner-text">
              <strong>Partly completed: {movedItems.length} of {items.length} items quarantined</strong>
              <span>
                {failedCount === 1 ? failedItem.name : `${failedItem.name} and ${plural(failedCount - 1, "other item")}`} {failedCount === 1 ? "was" : "were"} not moved:{" "}
                {props.failedText ?? "Kleari couldn’t move it."} {failedCount === 1 ? "It was" : "They were"} left where {failedCount === 1 ? "it was" : "they were"}. Nothing was deleted.
              </span>
            </div>
          </div>
        )}

        <div className="pv-grid">
          <div className="pv-col">
            <section aria-labelledby="pv-items" className="v2-card pv-items">
              <div className="v2-card-head">
                <h2 id="pv-items" className="v2-card-title">
                  {result
                    ? "What happened to each item"
                    : deleteMode && stage !== "review"
                      ? `${plural(items.length, "item")} to delete permanently`
                      : `${plural(items.length, "item")} selected`}
                </h2>
                {stage === "review" && <LinkButton href={hrefFor("review")} size="sm">Change selection</LinkButton>}
              </div>
              {/* The only part of the page that scrolls: the summary and actions stay in view. */}
              <ul className="pv-list" tabIndex={0} aria-label="Selected items">
                {items.map((item) => {
                  const meta = categoryMeta[item.category];
                  const effect = effects[states[item.id] ?? "planned"];
                  return (
                    <li key={item.id}>
                      <IconTile tone={meta.tone} icon={meta.icon} size={34} />
                      <span className="v2-row-text">
                        <span className="pv-name">
                          <span className="v2-ellipsis" title={item.name}>{item.name}</span>
                          <span className={`v2-pill v2-pill-tone tone-${meta.tone}`}>{meta.label}</span>
                        </span>
                        <span className="v2-mono pv-path" title={item.path}>{item.path}</span>
                      </span>
                      <span className="pv-end">
                        <span className="pv-size">{formatGb(item.gb)}</span>
                        <span className={`pv-effect ${effect.className}`}>
                          {effect.icon === "spin" ? <Spinner size={12} /> : <Glyph name={effect.icon} size={12} strokeWidth={2.4} />}
                          <span>{effect.text}</span>
                        </span>
                      </span>
                    </li>
                  );
                })}
              </ul>
            </section>

            <section aria-labelledby="pv-how" className="v2-card v2-card-pad pv-how">
              <h2 id="pv-how" className="v2-card-title">What happens when you approve</h2>
              <ol>
                {steps.map((step, index) => (
                  <li key={step.title}>
                    <span className={`pv-step-num tone-${step.tone}`} aria-hidden="true">{index + 1}</span>
                    <span className="v2-row-text">
                      <span className="pv-step-title">{step.title}</span>
                      <span className="pv-step-sub">{step.sub}</span>
                    </span>
                  </li>
                ))}
              </ol>
            </section>
          </div>

          <section aria-labelledby="pv-summary" className="v2-card pv-summary">
            <div className="pv-summary-head">
              <SummaryRing fraction={fraction} tone={ringTone} heroTone={heroTone} heroIcon={heroIcon} />
              <div className="pv-summary-text">
                <h2 id="pv-summary">{deletedResult ? "Deleted permanently" : result ? "Moved to quarantine" : "Ready"}</h2>
                <span className="pv-summary-value">{formatGb(deletedResult ? deletedGb : result ? movedGb : totalGb)}</span>
                <span className="v2-card-note">
                  {result
                    ? `${doneCount} of ${plural(items.length, "item")}${partlyDone ? ` · ${items.length - doneCount} ${deletedResult ? "not deleted" : "left in place"}` : ""}`
                    : `${plural(items.length, "item")} in ${plural(categories.length, "category", "categories")}`}
                </span>
              </div>
            </div>
            <div role="img" aria-label={`By category: ${categoryText}`} className="pv-cats">
              <div className="pv-cat-bar">
                {categories.map(([key, gb]) => (
                  <span key={key} className={`tone-${categoryMeta[key].tone}`} style={{ width: `${totalGb > 0 ? (gb / totalGb) * 100 : 0}%` }} />
                ))}
              </div>
              <div className="pv-cat-legend">
                {categories.map(([key, gb]) => (
                  <span key={key}><Dot tone={categoryMeta[key].tone} size={8} />{categoryMeta[key].label} {formatGb(gb)}</span>
                ))}
              </div>
            </div>
            <ul className="v2-facts">
              {deleteMode && stage !== "review" ? (
                <li><StatusMark tone="warn" /><span>Deleted items skip the Trash and can’t be restored</span></li>
              ) : (
                <>
                  <li><StatusMark tone="ok" /><span>Quarantine keeps every item restorable</span></li>
                  <li><StatusMark tone="ok" /><span>A restore never overwrites a file that is already there</span></li>
                </>
              )}
              <li><StatusMark tone="info" /><span>{props.consequences}</span></li>
            </ul>
            <div className="pv-folder">
              <span className="v2-muted">Quarantine folder</span>
              <span className="v2-mono">{props.quarantineFolder}</span>
            </div>

            {stage === "review" && (
              <>
                <label className={`v2-ack${acknowledged ? " on" : ""}`}>
                  <input type="checkbox" checked={acknowledged} onChange={() => setAcknowledged(!acknowledged)} />
                  <span>I reviewed these {plural(items.length, "item")}.</span>
                </label>
                <div className="pv-actions">
                  <LinkButton href={hrefFor("review")} size="lg" className="pv-back">Back</LinkButton>
                  <Button variant="primary" size="lg" className="pv-approve" disabled={!acknowledged} onClick={() => props.onApprove("quarantine")}>
                    Quarantine {plural(items.length, "item")}
                  </Button>
                </div>
                <Button
                  variant="dangerOutline"
                  className="pv-delete"
                  disabled={!acknowledged}
                  onClick={() => {
                    setDeleteAcknowledged(false);
                    setConfirmDelete(true);
                  }}
                >
                  Delete permanently instead…
                </Button>
              </>
            )}
            {stage === "running" && (
              <div role="status" className="pv-running">
                <Spinner size={16} />
                {deleteMode ? "Checking each item, then moving it before deleting…" : "Checking each item, then moving it…"}
              </div>
            )}
            {stage === "deleting" && (
              <div role="status" className="pv-running pv-running-danger">
                <Spinner size={16} tone="danger" />
                Deleting permanently…
              </div>
            )}
            {(stage === "stale" || stage === "error") && (
              <div className="pv-actions">
                <LinkButton href={hrefFor("review")} size="lg" className="pv-back">Back to results</LinkButton>
                <Button variant="primary" size="lg" className="pv-approve" disabled>Approval blocked</Button>
              </div>
            )}
            {result && (
              <div className="pv-actions">
                <LinkButton href={hrefFor("quarantine")} size="lg" className="pv-back">Open Quarantine</LinkButton>
                <LinkButton href={hrefFor("cleanup")} variant="primary" size="lg" className="pv-back">Done</LinkButton>
              </div>
            )}
          </section>
        </div>
      </ScrollBody>

      <Modal open={confirmDelete} onClose={() => setConfirmDelete(false)} labelledBy="pv-delete-title" role="alertdialog">
        <div className="v2-dialog-body">
          <div className="v2-dialog-head">
            <IconTile tone="red" icon="trashLines" size={48} glow />
            <div className="v2-row-text">
              <h2 id="pv-delete-title" className="v2-dialog-title">Delete {plural(items.length, "item")} permanently?</h2>
              <span className="pv-danger-text">This can’t be undone. They skip the Trash and can’t be restored.</span>
            </div>
          </div>
          <p className="v2-dialog-text">
            Kleari checks each item again as for quarantine, moves it into quarantine, then deletes it from there straight away ({formatGb(totalGb)}). Anything that can’t be deleted stays in Quarantine.
          </p>
          <label className={`v2-ack v2-ack-danger${deleteAcknowledged ? " on" : ""}`}>
            <input type="checkbox" checked={deleteAcknowledged} onChange={() => setDeleteAcknowledged(!deleteAcknowledged)} />
            <span>I understand these {plural(items.length, "item")} will be deleted for good.</span>
          </label>
          <div className="v2-dialog-actions">
            <Button variant="secondary" onClick={() => setConfirmDelete(false)}>Cancel</Button>
            <Button
              variant="danger"
              disabled={!deleteAcknowledged}
              onClick={() => {
                setConfirmDelete(false);
                props.onApprove("delete");
              }}
            >
              Delete permanently
            </Button>
          </div>
        </div>
      </Modal>
    </>
  );
}
