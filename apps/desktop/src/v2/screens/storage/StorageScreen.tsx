import { useEffect, useRef, useState } from "react";
import type { StorageExplorer as Explorer } from "../../../features/storage/useStorageExplorer";
import { StorageExplorer } from "./StorageExplorer";
import { Glyph } from "../../ui/icons";
import { Button, IconTile, Modal, PageHeader, Pill, ProgressBar, ScrollBody, Segmented, Spinner, Toast, useToast, type Status } from "../../ui/components";
import { formatGb } from "../../ui/format";
import { headingIdFor, tabFromHash } from "../../shell/routes";
import type { CleanupPreview } from "../../../contracts/cleanup";
import { useRemoval } from "../../../features/removal/RemovalProvider";
import type { AnalysisStep, FolderNode, FolderTag, LargeFile, ManagedSpace, StorageAnalysis, StorageCapacity, StorageCategoryDetail } from "./types";
import "./storage.css";
import { AccessSteps } from "../../ui/AccessSteps";

interface StorageScreenProps {
  volume: string;
  /** `null` when macOS didn't report the disk's capacity. */
  capacity: StorageCapacity | null;
  capacityNote: string | null;
  /** `null` until an analysis has finished. */
  tree: FolderNode | null;
  categories: readonly StorageCategoryDetail[];
  largeFiles: readonly LargeFile[];
  largeFilesNote: string;
  managed: readonly ManagedSpace[];
  analysis: StorageAnalysis;
  analysisSteps: readonly AnalysisStep[];
  analyzedNote: string | null;
  fullDiskAccess: "granted" | "notGranted" | "unknown";
  onAnalyze: () => void;
  onCancelAnalysis: () => void;
  /** Resolves to whether System Settings opened. */
  onOpenPrivacySettings: () => Promise<boolean>;
  /** Prepares a large file for removal here (the confirm sheet shows it). */
  onPrepareFile: (file: LargeFile) => Promise<CleanupPreview>;
  /** Shows a large file in Finder, so it can be looked at before it is removed. */
  onRevealFile: (file: LargeFile) => Promise<void>;
  /** Opens Cleanup with this scan chosen; it doesn't start the scan. */
  onScanInCleanup: (mode: "standard" | "deep" | "dev") => void;
  /** The Browse view: any folder of the home folder, measured on demand. */
  explorer: Explorer;
}

const tagMeta: Record<FolderTag, { label: string; status: Status }> = {
  macos: { label: "Managed by macOS", status: "muted" },
  yours: { label: "Your files", status: "link" },
};

const views = ["folders", "files", "browse"] as const;
type View = (typeof views)[number];

/**
 * Whether Analyze first explains Full Disk Access: only when it is known to be off, once per
 * visit, and never while an analysis is already running (the button then just shows it).
 */
export function shouldAskForAccess(access: "granted" | "notGranted" | "unknown", alreadyAsked: boolean, running: boolean): boolean {
  return access === "notGranted" && !alreadyAsked && !running;
}

/** A size that may be larger than shown, because something inside couldn't be read. */
function sizeText(gb: number, lowerBound: boolean | undefined): string {
  return lowerBound ? `at least ${formatGb(gb)}` : formatGb(gb);
}

export function StorageScreen(props: StorageScreenProps) {
  const { tree, categories, analysis, capacity } = props;
  const [view, setView] = useState<View>(() => tabFromHash(views, "folders"));
  const [path, setPath] = useState<readonly number[]>([]);
  const [categoryId, setCategoryId] = useState("developer");
  // Full Disk Access can't be requested by an app, so Kleari explains it once per visit
  // before analyzing without it, and offers the System Settings pane.
  const [accessAsked, setAccessAsked] = useState(false);
  const [accessOpen, setAccessOpen] = useState(false);
  const toast = useToast();
  const removal = useRemoval();
  const exploreRef = useRef<HTMLElement>(null);
  // Large files removed since this analysis, hidden until the next one.
  const [gone, setGone] = useState<ReadonlySet<string>>(new Set());

  // The analysis runs on the page, not in a dialog; say so once it finishes.
  const wasRunning = useRef(analysis.running);
  const { show } = toast;
  useEffect(() => {
    if (wasRunning.current && !analysis.running && !analysis.stopped && !analysis.failed) {
      show(`Storage analysis updated${analysis.doneNote ? `: ${analysis.doneNote}` : ""}. Nothing was changed.`);
    }
    wasRunning.current = analysis.running;
  }, [analysis.running, analysis.stopped, analysis.failed, analysis.doneNote, show]);

  const category = categories.find((entry) => entry.id === categoryId) ?? categories[0];
  const barTotal = capacity?.totalGb ?? Math.max(categories.reduce((total, entry) => total + entry.gb, 0), 1);
  const startAnalysis = () => {
    if (shouldAskForAccess(props.fullDiskAccess, accessAsked, analysis.running)) {
      setAccessOpen(true);
      return;
    }
    props.onAnalyze();
  };
  const openPrivacySettings = () => {
    void props.onOpenPrivacySettings().then((opened) => {
      if (!opened) {
        toast.show("Couldn’t open System Settings. Open Privacy & Security, then Full Disk Access.");
      }
    });
  };
  const revealFile = (file: LargeFile) => {
    props.onRevealFile(file).catch(() => toast.show(`Couldn’t show ${file.name} in Finder. It may have moved since the analysis.`));
  };
  const showFolders = (browse: readonly number[]) => {
    setView("folders");
    setPath(browse);
    exploreRef.current?.scrollIntoView({ block: "start" });
  };

  return (
    <>
      <PageHeader
        title="Storage"
        headingId={headingIdFor("storage")}
        subtitle="See what is using space on this Mac. This screen only reads; nothing is moved or deleted from here."
        actions={
          <>
            <span className="st-volume"><Glyph name="disk" size={14} />{props.volume}</span>
            {analysis.running ? (
              <Button variant="secondary" size="sm" onClick={props.onCancelAnalysis}>
                <Spinner size={10} /> Cancel
              </Button>
            ) : (
              <Button variant="soft" size="sm" onClick={startAnalysis}>{tree ? "Analyze again" : "Analyze"}</Button>
            )}
          </>
        }
      />
      <ScrollBody label="Storage" className="st-body">
        <section aria-labelledby="st-cap" className="v2-card st-capacity">
          <div className="st-cap-head">
            <div className="st-cap-total">
              <h2 id="st-cap">Used on {props.volume}</h2>
              {capacity ? (
                <span>{Math.round(capacity.usedGb)} GB <span>of {Math.round(capacity.totalGb)} GB</span></span>
              ) : (
                <span><span>Not reported</span></span>
              )}
            </div>
            {capacity && (
              <span className="st-available">Available <strong className="st-ok v2-num">{Math.round(capacity.availableGb)} GB</strong></span>
            )}
          </div>
          {props.capacityNote && <span className="v2-card-note">{props.capacityNote}</span>}
          <div
            className="st-bar"
            role="img"
            aria-label={
              categories.length === 0
                ? "Not analyzed yet"
                : capacity
                  ? `${Math.round(capacity.usedGb)} GB used of ${Math.round(capacity.totalGb)} GB, split by category`
                  : "Counted space, split by category"
            }
          >
            {categories.map((entry) => (
              <span
                key={entry.id}
                title={entry.lowerBound ? `${entry.label} (at least)` : entry.label}
                className={`tone-${entry.tone}${entry.id === category?.id ? " on" : ""}`}
                style={{ width: `${Math.min((entry.gb / barTotal) * 100, 100).toFixed(2)}%` }}
              />
            ))}
          </div>
          {categories.length > 0 && (
            <div role="group" aria-label="Categories" className="st-cats">
              {categories.map((entry) => (
                <button
                  key={entry.id}
                  type="button"
                  aria-pressed={entry.id === category?.id}
                  className={`st-cat tone-${entry.tone}${entry.id === category?.id ? " on" : ""}`}
                  onClick={() => setCategoryId(entry.id)}
                >
                  <span className="st-cat-dot" aria-hidden="true" />
                  <span className="st-cat-label">{entry.label}</span>
                  <span className="v2-muted v2-num">{sizeText(entry.gb, entry.lowerBound)}</span>
                </button>
              ))}
            </div>
          )}
          <AnalysisStatus analysis={analysis} steps={props.analysisSteps} usedGb={capacity?.usedGb ?? null} hasResult={tree !== null} />
          {!analysis.running && props.analyzedNote && <span className="v2-card-note">{props.analyzedNote}</span>}
          {props.fullDiskAccess === "notGranted" && (
            <div className="st-access">
              <span className="st-access-head">
                <Glyph name="lock" size={15} />
                Some folders can only be measured with Full Disk Access.
              </span>
              <AccessSteps kind="fullDiskAccess" compact />
              <span>
                <Button variant="soft" size="sm" onClick={openPrivacySettings}>Open System Settings</Button>
              </span>
            </div>
          )}
        </section>

        <div className={`st-pair${category ? "" : " st-pair-one"}`}>
          {category && (
            <section aria-labelledby="st-category" className="v2-card st-detail">
              <div className="st-detail-head">
                <IconTile tone={category.tone} icon={category.icon} size={28} />
                <span className="v2-row-text">
                  <h2 id="st-category" className="st-card-title">{category.label}</h2>
                  <span className="v2-muted v2-num st-detail-sub">
                    {sizeText(category.gb, category.lowerBound)}
                    {capacity &&
                      capacity.usedGb > 0 &&
                      ` · ${category.lowerBound ? "at least " : ""}${Math.round((category.gb / capacity.usedGb) * 100)}% of used space`}
                  </span>
                </span>
                <Pill status={category.pillStatus}>{category.pill}</Pill>
              </div>
              <span className="st-blurb">{category.blurb}</span>
              {category.top.length > 0 && (
                <ul className="v2-info-list">
                  {category.top.map((entry) => (
                    <li key={entry.name}><span className="v2-ellipsis">{entry.name}</span><strong className="v2-num">{formatGb(entry.gb)}</strong></li>
                  ))}
                </ul>
              )}
              {(category.browse || category.cleanupMode) && (
                <div className="st-detail-foot">
                  {category.browse && (
                    <Button variant="soft" size="sm" onClick={() => showFolders(category.browse ?? [])}>Show folders</Button>
                  )}
                  {category.cleanupMode && (
                    <Button variant="soft" size="sm" onClick={() => category.cleanupMode && props.onScanInCleanup(category.cleanupMode)}>Scan in Cleanup</Button>
                  )}
                </div>
              )}
            </section>
          )}

          <section aria-labelledby="st-managed" className="v2-card st-managed">
            <div>
              <h2 id="st-managed" className="st-card-title">Space macOS manages</h2>
              <span className="v2-card-note">Shown for context. Kleari doesn't remove any of these.</span>
            </div>
            {props.managed.map((entry) => (
              <div key={entry.label} className="st-managed-row">
                <IconTile tone={entry.tone} icon={entry.icon} size={26} />
                <span className="v2-row-text">
                  <span className="st-managed-label">{entry.label}</span>
                  <span className="v2-row-sub">{entry.sub}</span>
                </span>
                <span className="v2-row-size">{entry.gb === null ? "Not reported" : formatGb(entry.gb)}</span>
              </div>
            ))}
          </section>
        </div>

        <section ref={exploreRef} aria-labelledby="st-explore" className="v2-card st-explore">
          <div className="st-explore-head">
            <h2 id="st-explore" className="st-card-title">What is taking space</h2>
            <Segmented
              label="View"
              value={view}
              onChange={setView}
              options={[
                { value: "folders", label: "Largest folders" },
                { value: "files", label: "Large files" },
                { value: "browse", label: "Browse" },
              ]}
            />
          </div>
          {view === "browse" ? (
            <StorageExplorer explorer={props.explorer} />
          ) : !tree ? (
            <div className="st-empty">
              {analysis.running ? <Spinner size={18} /> : <IconTile tone="blue" icon="disk" size={32} />}
              <span role="status" className="v2-row-title">{analysis.running ? "Analyzing…" : "Not analyzed yet"}</span>
              <span className="v2-card-note">
                {analysis.running
                  ? "Results show here when it’s done. You can keep using Kleari meanwhile."
                  : "Analyzing measures Applications, your home folder and the system folders. It only reads sizes. macOS may ask before Kleari can read some of your folders."}
              </span>
              {!analysis.running && <Button variant="primary" size="sm" onClick={startAnalysis}>Analyze</Button>}
            </div>
          ) : view === "folders" ? (
            <FolderList tree={tree} path={path} onPathChange={setPath} />
          ) : (
            <>
              <span className="v2-card-note st-files-note">
                {props.largeFilesNote} A file in your home folder can be removed here; you'll see it again before it goes. Elsewhere it's there to inspect.
              </span>
              {props.largeFiles.length === 0 ? (
                <span className="v2-card-note st-files-note">No files of 1 GB or more were found.</span>
              ) : (
                <ul aria-label="Large files" className="st-files">
                  {props.largeFiles.filter((file) => !gone.has(file.reviewId)).map((file) => (
                    <li key={file.id}>
                      <IconTile tone={file.tone} icon={file.icon} size={26} />
                      <span className="v2-row-text">
                        <span className="v2-row-title">{file.name}</span>
                        <span className="v2-row-sub st-file-where"><span className="v2-mono">{file.path}</span> · {file.age}</span>
                      </span>
                      <span className="st-file-size">{formatGb(file.gb)}</span>
                      <span className="st-file-action">
                        <button
                          type="button"
                          className="st-reveal"
                          aria-label={`Show ${file.name} in Finder`}
                          title="Show in Finder"
                          onClick={() => revealFile(file)}
                        >
                          <Glyph name="folder" size={14} strokeWidth={2} />
                        </button>
                        {file.locked ? (
                          <Pill status="muted"><span title={file.locked.tip}>{file.locked.label}</span></Pill>
                        ) : (
                          <Button
                            variant="soft"
                            size="sm"
                            aria-label={`Remove ${file.name}`}
                            onClick={() =>
                              removal({
                                what: file.name,
                                prepare: () => props.onPrepareFile(file),
                                onDone: (count) => count > 0 && setGone((current) => new Set([...current, file.reviewId])),
                              })
                            }
                          >
                            Remove…
                          </Button>
                        )}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
            </>
          )}
        </section>
      </ScrollBody>

      <Toast message={toast.message} />

      <Modal open={accessOpen} onClose={() => setAccessOpen(false)} labelledBy="st-access-title" className="st-an">
        <div className="v2-dialog-body st-an-body">
          <div className="v2-dialog-head">
            <IconTile tone="blue" icon="lock" size={48} glow />
            <div className="v2-row-text">
              <h2 id="st-access-title" className="v2-dialog-title">Measure everything with Full Disk Access?</h2>
              <span className="v2-dialog-text">
                Without it, macOS doesn’t let Kleari read folders such as Mail, Messages and some app data, so their space shows as Not measured. Apps can’t ask for this access directly: turn on Kleari in Privacy &amp; Security › Full Disk Access, then open Kleari again.
              </span>
            </div>
          </div>
          <div className="st-an-note">
            <Glyph name="shield" size={15} />
            <span>Kleari would still only read sizes. Nothing is moved or deleted.</span>
          </div>
          <AccessSteps kind="fullDiskAccess" compact />
          <div className="v2-dialog-actions-end">
            <Button
              variant="secondary"
              onClick={() => {
                setAccessOpen(false);
                setAccessAsked(true);
                props.onAnalyze();
              }}
            >
              Analyze without it
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                setAccessOpen(false);
                setAccessAsked(true);
                openPrivacySettings();
              }}
            >
              Open System Settings
            </Button>
          </div>
        </div>
      </Modal>
    </>
  );
}

/** The analysis's progress, or why it ended early, shown on the page while you carry on. */
function AnalysisStatus({ analysis, steps, usedGb, hasResult }: { analysis: StorageAnalysis; steps: readonly AnalysisStep[]; usedGb: number | null; hasResult: boolean }) {
  const { running, stopped, failed, percent, stepIndex, countedGb } = analysis;
  if (!running) {
    if (!failed && !stopped) {
      return null;
    }
    const why = failed ?? `Analysis stopped. ${hasResult ? "Showing the results from your last analysis." : "Nothing was counted."}`;
    return (
      <span role="status" className="st-ended">
        <Glyph name="warning" size={13} />
        {why}
      </span>
    );
  }
  // Without used space there is no byte denominator, so progress is finished steps only.
  const stepText = `Step ${Math.min(stepIndex + 1, steps.length)} of ${steps.length}`;
  const barPercent = percent ?? (stepIndex / Math.max(steps.length, 1)) * 100;
  return (
    <div className="st-progress">
      <ProgressBar percent={Math.max(barPercent, 2)} label="Storage analysis" valueText={percent === null ? stepText : `${percent}%`} height={6} />
      <div className="st-progress-text" role="status" aria-live="polite">
        <span>{steps[stepIndex]?.label ?? "Reading"}… · {stepText}</span>
        <span className="v2-num">
          {usedGb === null ? `${formatGb(countedGb)} counted` : `${formatGb(countedGb)} of ${Math.round(usedGb)} GB counted`}
          {percent !== null && ` · ${percent}%`}
        </span>
      </div>
    </div>
  );
}

function FolderList({ tree, path, onPathChange }: { tree: FolderNode; path: readonly number[]; onPathChange: (path: readonly number[]) => void }) {
  const chain: FolderNode[] = [tree];
  for (const index of path) {
    const next = chain[chain.length - 1].children?.[index];
    if (next) {
      chain.push(next);
    }
  }
  const current = chain[chain.length - 1];
  const children = (current.children ?? []).map((child, index) => ({ child, index })).sort((a, b) => b.child.gb - a.child.gb);
  const maxGb = Math.max(...children.map((entry) => entry.child.gb), 1e-9);
  return (
    <>
      <nav aria-label="Folder path" className="st-crumbs">
        {chain.map((entry, depth) => (
          <span key={depth} className="st-crumb">
            {depth > 0 && <Glyph name="chevronRight" size={12} />}
            {depth === chain.length - 1 ? (
              <span aria-current="location" className="st-crumb-here">{entry.name}</span>
            ) : (
              <button type="button" className="st-crumb-link" onClick={() => onPathChange(path.slice(0, depth))}>{entry.name}</button>
            )}
          </span>
        ))}
        <span className="st-crumb-total v2-num">{sizeText(current.gb, current.lowerBound)} total</span>
      </nav>
      <ul aria-label={`Folders in ${current.name}`} className="st-folders">
        {children.map(({ child, index }) => {
          const canOpen = child.children !== undefined;
          const tag = child.tag ? tagMeta[child.tag] : undefined;
          const size = sizeText(child.gb, child.lowerBound);
          return (
            <li key={`${index}-${child.name}`}>
              <button
                type="button"
                className="st-folder"
                disabled={!canOpen}
                aria-label={canOpen ? `Open ${child.name}, ${size}` : `${child.name}, ${size}`}
                onClick={() => canOpen && onPathChange([...path, index])}
              >
                <IconTile tone={child.tone} icon={child.icon} size={26} />
                <span className="st-folder-main">
                  <span className="st-folder-name">
                    <span className="v2-ellipsis">{child.name}</span>
                    {tag && <Pill status={tag.status} className="st-tag">{tag.label}</Pill>}
                  </span>
                  <span className="st-folder-track" aria-hidden="true">
                    <span className={`tone-${child.tone}`} style={{ width: `${((child.gb / maxGb) * 100).toFixed(1)}%` }} />
                  </span>
                </span>
                <span className="st-folder-size" title={child.lowerBound ? "At least this much: something inside couldn’t be read" : undefined}>
                  {child.lowerBound ? "≥ " : ""}{formatGb(child.gb)}
                </span>
                <span className="st-folder-chevron">{canOpen && <Glyph name="chevronRight" size={14} />}</span>
              </button>
            </li>
          );
        })}
      </ul>
    </>
  );
}
