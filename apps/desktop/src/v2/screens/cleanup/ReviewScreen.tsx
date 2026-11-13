import { useState } from "react";
import { Glyph, StatusMark } from "../../ui/icons";
import { Button, FloatingBar, IconTile, LinkButton, Modal, PageHeader, Pill, SearchField, Switch, Toast, useToast } from "../../ui/components";
import { formatGb, sum } from "../../ui/format";
import { headingIdFor, hrefFor } from "../../shell/routes";
import { CleanupSteps } from "./CleanupSteps";
import { categoryMeta, eligibilityIcon, eligibilityLabel, riskStatus } from "./findingMeta";
import { isSelectable, type Finding, type FindingCategory } from "./types";
import "./review.css";

const PAGE_SIZE = 10;
const categoryOrder: readonly FindingCategory[] = ["cache", "xcode", "pkg", "node", "build", "logs", "apps", "chosen", "large", "old"];

interface ReviewScreenProps {
  findings: readonly Finding[];
  categoryCounts: Record<FindingCategory, number>;
  subtitle: string;
  selection: ReadonlySet<string>;
  onSelectionChange: (next: ReadonlySet<string>) => void;
  initialQuery: string;
  /** Shows the item in Finder; resolves to a message when it couldn't. */
  onRevealInFinder: (finding: Finding) => Promise<string | null>;
  /** Why the review couldn't load, if it didn't. */
  error?: string | null;
}

function EligibilityText({ finding }: { finding: Finding }) {
  return (
    <span className={`rev-elig rev-elig-${finding.eligibility}`}>
      <Glyph name={eligibilityIcon[finding.eligibility]} size={13} strokeWidth={2.4} />
      {eligibilityLabel[finding.eligibility]}
    </span>
  );
}

export function ReviewScreen({ findings, categoryCounts, subtitle, selection, onSelectionChange, initialQuery, onRevealInFinder, error }: ReviewScreenProps) {
  const [category, setCategory] = useState<FindingCategory | "all">("all");
  const [eligibleOnly, setEligibleOnly] = useState(false);
  const [query, setQuery] = useState(initialQuery);
  const [page, setPage] = useState(1);
  const [openId, setOpenId] = useState<string | null>(null);
  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set());
  const toast = useToast();

  const needle = query.trim().toLowerCase();
  const shown = findings.filter(
    (finding) =>
      !hidden.has(finding.id) &&
      (category === "all" || finding.category === category) &&
      (!eligibleOnly || isSelectable(finding)) &&
      (needle === "" || `${finding.name} ${finding.path}`.toLowerCase().includes(needle)),
  );
  const pageCount = Math.max(1, Math.ceil(shown.length / PAGE_SIZE));
  const currentPage = Math.min(page, pageCount);
  const rows = shown.slice((currentPage - 1) * PAGE_SIZE, currentPage * PAGE_SIZE);
  const picked = findings.filter((finding) => selection.has(finding.id) && isSelectable(finding));
  const pickedGb = sum(picked, (finding) => finding.gb);
  const eligibleShown = shown.filter(isSelectable);
  const allOn = eligibleShown.length > 0 && eligibleShown.every((finding) => selection.has(finding.id));
  const open = findings.find((finding) => finding.id === openId);

  const toggle = (id: string) => {
    const next = new Set(selection);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    onSelectionChange(next);
  };

  const toggleAll = () => {
    const next = new Set(selection);
    for (const finding of eligibleShown) {
      if (allOn) {
        next.delete(finding.id);
      } else {
        next.add(finding.id);
      }
    }
    onSelectionChange(next);
  };

  const resetPage = () => setPage(1);
  const total = sum(categoryOrder, (key) => categoryCounts[key]);

  return (
    <>
      <PageHeader above={<CleanupSteps current={1} />} title="Review results" headingId={headingIdFor("review")} subtitle={subtitle} />

      <div className="rev-filters">
        <div role="group" aria-label="Category" className="rev-chips">
          <button
            type="button"
            aria-pressed={category === "all"}
            className={`rev-chip rev-chip-all${category === "all" ? " on" : ""}`}
            onClick={() => { setCategory("all"); resetPage(); }}
          >
            <span className="v2-dot rev-dot-all" aria-hidden="true" />
            <span>All results</span>
            <span className="rev-chip-count">{total}</span>
          </button>
          {categoryOrder.filter((key) => categoryCounts[key] > 0).map((key) => (
            <button
              key={key}
              type="button"
              aria-pressed={category === key}
              className={`rev-chip tone-${categoryMeta[key].tone}${category === key ? " on" : ""}`}
              onClick={() => { setCategory(key); resetPage(); }}
            >
              <span className="v2-dot" aria-hidden="true" />
              <span>{categoryMeta[key].label}</span>
              <span className="rev-chip-count">{categoryCounts[key]}</span>
            </button>
          ))}
        </div>
        <SearchField label="Search results" value={query} onChange={(value) => { setQuery(value); resetPage(); }} width={190} />
      </div>

      <div className="rev-toolbar">
        <label className="rev-elig-toggle">
          <Switch size="sm" label="Show eligible items only" checked={eligibleOnly} onChange={(on) => { setEligibleOnly(on); resetPage(); }} />
          <span aria-hidden="true">Eligible only</span>
        </label>
        <nav aria-label="Result pages" className="rev-pages">
          <span role="status" className="v2-muted v2-num">
            {shown.length ? `Showing ${(currentPage - 1) * PAGE_SIZE + 1}–${(currentPage - 1) * PAGE_SIZE + rows.length} of ${shown.length}` : "Showing 0 of 0"}
          </span>
          <div className="rev-page-buttons">
            <button type="button" aria-label="Previous page" disabled={currentPage <= 1} className="rev-page-arrow" onClick={() => setPage(Math.max(1, currentPage - 1))}>
              <Glyph name="chevronLeft" size={12} strokeWidth={2.6} />
            </button>
            {Array.from({ length: pageCount }, (_, index) => (
              <button
                key={index}
                type="button"
                aria-label={`Page ${index + 1}`}
                aria-current={index + 1 === currentPage ? "page" : undefined}
                className={`rev-page${index + 1 === currentPage ? " on" : ""}`}
                onClick={() => setPage(index + 1)}
              >
                {index + 1}
              </button>
            ))}
            <button type="button" aria-label="Next page" disabled={currentPage >= pageCount} className="rev-page-arrow" onClick={() => setPage(Math.min(pageCount, currentPage + 1))}>
              <Glyph name="chevronRight" size={12} strokeWidth={2.6} />
            </button>
          </div>
        </nav>
      </div>

      <div className="rev-table-wrap" tabIndex={0} role="region" aria-label="Scan results">
        <table className="rev-table">
          <thead>
            <tr>
              <th scope="col" className="v2-th-glass rev-col-check">
                <input type="checkbox" aria-label="Select all eligible items" checked={allOn} disabled={eligibleShown.length === 0} onChange={toggleAll} />
              </th>
              <th scope="col" className="v2-th-glass">Item</th>
              <th scope="col" className="v2-th-glass">Category</th>
              <th scope="col" className="v2-th-glass rev-col-size" aria-sort="descending">Size ↓</th>
              <th scope="col" className="v2-th-glass">Risk</th>
              <th scope="col" className="v2-th-glass">Eligibility</th>
              <th scope="col" className="v2-th-glass rev-col-more"><span className="v2-visually-hidden">Details</span></th>
            </tr>
          </thead>
          <tbody>
            {rows.map((finding) => {
              const meta = categoryMeta[finding.category];
              const selectable = isSelectable(finding);
              const on = selectable && selection.has(finding.id);
              return (
                <tr key={finding.id} className={openId === finding.id ? "open" : on ? "selected" : ""}>
                  <td className="rev-col-check">
                    <input
                      type="checkbox"
                      checked={on}
                      disabled={!selectable}
                      aria-label={`${selectable ? "Select" : "Cannot select"} ${finding.name}`}
                      onChange={() => toggle(finding.id)}
                    />
                  </td>
                  <td className="rev-col-item">
                    <div className="rev-item">
                      <span style={{ opacity: selectable ? 1 : 0.6, display: "flex" }}>
                        <IconTile tone={meta.tone} icon={meta.icon} size={30} />
                      </span>
                      <span className="v2-row-text">
                        <button type="button" className="rev-name" onClick={() => setOpenId(finding.id)}>{finding.name}</button>
                        <span className="rev-reason">{finding.reason}</span>
                      </span>
                    </div>
                  </td>
                  <td><span className={`v2-pill v2-pill-tone tone-${meta.tone}`}>{meta.label}</span></td>
                  <td className="rev-col-size">{formatGb(finding.gb)}</td>
                  <td><Pill status={riskStatus[finding.risk]}>{finding.risk} risk</Pill></td>
                  <td><EligibilityText finding={finding} /></td>
                  <td className="rev-col-more">
                    <button type="button" className="rev-more" aria-label={`Details for ${finding.name}`} onClick={() => setOpenId(finding.id)}>
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
                        <circle cx="5" cy="12" r="1.8" /><circle cx="12" cy="12" r="1.8" /><circle cx="19" cy="12" r="1.8" />
                      </svg>
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
        {rows.length === 0 &&
          (findings.length === 0 ? (
            <div className="rev-empty">
              <strong>{error ? "The review couldn’t load" : "Nothing to review yet"}</strong>
              <span>{error ?? "Run a scan in Cleanup, or send items from the Storage or Developer page."}</span>
              {!error && <LinkButton href={hrefFor("cleanup")} size="sm">Go to Cleanup</LinkButton>}
            </div>
          ) : (
            <div className="rev-empty">
              <strong>No items match these filters</strong>
              <span>The review has {findings.length} items. Clear the search or pick another category.</span>
            </div>
          ))}
      </div>

      <FloatingBar attached>
        <span className={`rev-sel-tile`} aria-hidden="true">
          <IconTile tone={picked.length ? "blue" : "slate"} icon="quarantine" size={34} />
        </span>
        <div className="v2-float-text">
          <span role="status" className="v2-float-title">{picked.length ? `${picked.length} selected · ${formatGb(pickedGb)}` : "Nothing selected"}</span>
          <span className="v2-float-sub">Selecting does nothing on its own. You'll see an exact preview first.</span>
        </div>
        <div className="v2-float-actions">
          <Button variant="soft" onClick={() => onSelectionChange(new Set())} disabled={picked.length === 0} className="rev-clear">Clear selection</Button>
          {picked.length > 0 ? (
            <LinkButton href={hrefFor("preview")} variant="primary">Create preview →</LinkButton>
          ) : (
            <Button variant="primary" disabled>Create preview →</Button>
          )}
        </div>
      </FloatingBar>

      <Toast message={toast.message} bottom={92} />

      <Modal open={open !== undefined} onClose={() => setOpenId(null)} labelledBy="rev-drawer-title" variant="drawer">
        {open && (
          <FindingDrawer
            finding={open}
            selected={selection.has(open.id)}
            onToggle={() => toggle(open.id)}
            onClose={() => setOpenId(null)}
            onReveal={() => {
              void onRevealInFinder(open).then((message) => message && toast.show(message, "warn"));
            }}
            onCopy={() => {
              void navigator.clipboard.writeText(open.path).then(
                () => toast.show("Copied the path"),
                () => toast.show("Couldn’t copy the path", "warn"),
              );
            }}
            onHide={() => {
              setHidden(new Set(hidden).add(open.id));
              setOpenId(null);
              toast.show(`Hid ${open.name} from these results. Nothing changed on disk.`, "info");
            }}
          />
        )}
      </Modal>
    </>
  );
}

interface FindingDrawerProps {
  finding: Finding;
  selected: boolean;
  onToggle: () => void;
  onClose: () => void;
  onReveal: () => void;
  onCopy: () => void;
  onHide: () => void;
}

function FindingDrawer({ finding, selected, onToggle, onClose, onReveal, onCopy, onHide }: FindingDrawerProps) {
  const meta = categoryMeta[finding.category];
  const selectable = isSelectable(finding);
  const stats = [
    { label: "Size", value: formatGb(finding.gb) },
    { label: "Files", value: finding.files },
    { label: "Last changed", value: finding.modified },
  ];
  return (
    <div className="rev-drawer">
      <div className="rev-drawer-head">
        <IconTile tone={meta.tone} icon={meta.icon} size={48} glow />
        <div className="rev-drawer-title">
          <h2 id="rev-drawer-title">{finding.name}</h2>
          <span className={`v2-pill v2-pill-tone tone-${meta.tone}`}>{meta.label}</span>
        </div>
        <button type="button" className="v2-close rev-drawer-close" aria-label="Close details" onClick={onClose}>
          <Glyph name="close" size={12} strokeWidth={2.6} />
        </button>
      </div>
      <div className="rev-drawer-stats">
        {stats.map((stat) => (
          <div key={stat.label}>
            <span className="rev-stat-value">{stat.value}</span>
            <span className="rev-stat-label">{stat.label}</span>
          </div>
        ))}
      </div>
      <div className="rev-drawer-section">
        <h3>Why Kleari found this</h3>
        <ul className="rev-evidence">
          {finding.evidence.map((evidence) => (
            <li key={evidence.text}>
              <StatusMark tone={evidence.tone} />
              <span>{evidence.text}</span>
            </li>
          ))}
        </ul>
      </div>
      <dl className="rev-drawer-facts">
        <dt>Certainty</dt>
        <dd className="rev-strong">{finding.certainty}</dd>
        <dt>Risk</dt>
        <dd><Pill status={riskStatus[finding.risk]}>{finding.risk} risk</Pill></dd>
        <dt>Eligibility</dt>
        <dd className={`rev-strong rev-elig-${finding.eligibility}`}>{finding.eligibilityLong}</dd>
        <dt>Location</dt>
        <dd className="v2-mono rev-path">{finding.path}</dd>
      </dl>
      <div className="rev-drawer-actions">
        {selectable ? (
          <Button variant={selected ? "soft" : "primary"} onClick={onToggle}>{selected ? "Remove from selection" : "Add to selection"}</Button>
        ) : (
          <div className="rev-why-not">{finding.whyNot}</div>
        )}
        <div className="rev-drawer-tools">
          <button type="button" onClick={onReveal}><Glyph name="folder" />Show in Finder</button>
          <button type="button" onClick={onCopy}><Glyph name="copy" />Copy path</button>
          <button type="button" onClick={onHide}><Glyph name="eyeOff" />Hide from results</button>
        </div>
      </div>
    </div>
  );
}
