import { useState, type ReactNode } from "react";
import { Glyph, StatusMark } from "../../ui/icons";
import { Button, IconTile, Modal, PageHeader, Pill, ProgressBar, ScrollEdgeGlass, Segmented, Spinner, Toast, useScrolled, useToast, type Status } from "../../ui/components";
import { formatGb } from "../../ui/format";
import { headingIdFor } from "../../shell/routes";
import type { AnalysisStep, FolderNode, FolderTag, LargeFile, ManagedSpace, StorageAnalysis, StorageCapacity, StorageCategoryDetail } from "./types";
import "./storage.css";

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
  /** Review IDs of large files already added to Cleanup's review. */
  inReview: ReadonlySet<string>;
  /** Adds a large file to Cleanup's review (to inspect); resolves to a message. Nothing moves. */
  onAddToReview: (file: LargeFile) => Promise<string>;
  /** Opens Cleanup with this scan chosen; it doesn't start the scan. */
  onScanInCleanup: (mode: "standard" | "deep" | "dev") => void;
}

const tagMeta: Record<FolderTag, { label: string; status: Status }> = {
  macos: { label: "Managed by macOS", status: "muted" },
  yours: { label: "Your files", status: "link" },
};

type View = "folders" | "files";

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
  const [view, setView] = useState<View>("folders");
  const [path, setPath] = useState<readonly number[]>([]);
  const [categoryId, setCategoryId] = useState("developer");
  const [analysisOpen, setAnalysisOpen] = useState(false);
  // Full Disk Access can't be requested by an app, so Kleari explains it once per visit
  // before analyzing without it, and offers the System Settings pane.
  const [accessAsked, setAccessAsked] = useState(false);
  const [accessOpen, setAccessOpen] = useState(false);
  const toast = useToast();

  const category = categories.find((entry) => entry.id === categoryId) ?? categories[0];
  const barTotal = capacity?.totalGb ?? Math.max(categories.reduce((total, entry) => total + entry.gb, 0), 1);
  const analyzeNow = () => {
    setAnalysisOpen(true);
    props.onAnalyze();
  };
  const startAnalysis = () => {
    if (shouldAskForAccess(props.fullDiskAccess, accessAsked, analysis.running)) {
      setAccessOpen(true);
      return;
    }
    analyzeNow();
  };
  const openPrivacySettings = () => {
    void props.onOpenPrivacySettings().then((opened) => {
      if (!opened) {
        toast.show("Couldn’t open System Settings. Open Privacy & Security, then Full Disk Access.");
      }
    });
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
            <Button variant="soft" size="sm" disabled={analysisOpen} onClick={startAnalysis}>
              {analysis.running && <Spinner size={10} />}
              {analysis.running ? "Analyzing…" : tree ? "Analyze again" : "Analyze"}
            </Button>
          </>
        }
      />
      <div className="st-body">
        <section aria-labelledby="st-cap" className="v2-card v2-card-pad st-capacity">
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
              <div className="st-chips">
                <span className="st-chip"><span>Available</span><strong className="st-ok">{Math.round(capacity.availableGb)} GB</strong></span>
              </div>
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
          {props.analyzedNote && <span className="v2-card-note">{props.analyzedNote}</span>}
          {props.fullDiskAccess === "notGranted" && (
            <div className="st-access">
              <Glyph name="lock" size={15} />
              <span>Some folders can only be measured with Full Disk Access. Kleari would still only read them.</span>
              <Button variant="soft" size="sm" onClick={openPrivacySettings}>Open Privacy Settings</Button>
            </div>
          )}
        </section>

        <div className="st-grid">
          <section aria-labelledby="st-explore" className="v2-card st-explore">
            <div className="st-explore-head">
              <h2 id="st-explore" className="v2-card-title-sm">What is taking space</h2>
              {tree && <Segmented label="View" value={view} onChange={setView} options={[{ value: "folders", label: "Largest folders" }, { value: "files", label: "Large files" }]} />}
            </div>
            {!tree ? (
              <div className="st-empty">
                {analysis.running ? <Spinner size={34} /> : <IconTile tone="blue" icon="disk" size={42} glow />}
                <span role="status" className="v2-row-title">{analysis.running ? "Analyzing…" : "Not analyzed yet"}</span>
                <span className="v2-card-note">
                  Analyzing measures Applications, your home folder and the system folders. It only reads sizes. macOS may ask before Kleari can read some of your folders.
                </span>
                {!analysis.running && <Button variant="primary" size="sm" onClick={startAnalysis}>Analyze</Button>}
              </div>
            ) : view === "folders" ? (
              <FolderList tree={tree} path={path} onPathChange={setPath} />
            ) : (
              <>
                <span className="v2-card-note st-files-note">
                  {props.largeFilesNote} Adding one to review lists it in Cleanup step 2 for you to inspect; Kleari never moves your files.
                </span>
                {props.largeFiles.length === 0 ? (
                  <span className="v2-card-note st-files-note">No files of 1 GB or more were found.</span>
                ) : (
                  <ScrollList label="Large files" className="st-files">
                    {props.largeFiles.map((file) => {
                      const added = props.inReview.has(file.reviewId);
                      return (
                        <li key={file.id}>
                          <IconTile tone={file.tone} icon={file.icon} size={30} />
                          <span className="v2-row-text">
                            <span className="v2-row-title">{file.name}</span>
                            <span className="v2-row-sub"><span className="v2-mono">{file.path}</span> · {file.age}</span>
                          </span>
                          <span className="st-file-size">{formatGb(file.gb)}</span>
                          <span className="st-file-action">
                            {file.locked ? (
                              <Pill status="muted"><span title={file.locked.tip}>{file.locked.label}</span></Pill>
                            ) : added ? (
                              <Pill status="ok">In review</Pill>
                            ) : (
                              <Button
                                variant="soft"
                                size="sm"
                                aria-label={`Add ${file.name} to review`}
                                onClick={() => {
                                  void props.onAddToReview(file).then((message) => toast.show(message));
                                }}
                              >
                                Add to review
                              </Button>
                            )}
                          </span>
                        </li>
                      );
                    })}
                  </ScrollList>
                )}
              </>
            )}
          </section>

          <div className="st-side">
            {category && (
              <section aria-labelledby="st-category" className="v2-card st-detail">
                <div className="st-detail-head">
                  <IconTile tone={category.tone} icon={category.icon} size={42} glow />
                  <span className="v2-row-text">
                    <h2 id="st-category" className="v2-card-title-sm">{category.label}</h2>
                    <span className="v2-muted v2-num st-detail-sub">
                      {sizeText(category.gb, category.lowerBound)}
                      {capacity &&
                        capacity.usedGb > 0 &&
                        ` · ${category.lowerBound ? "at least " : ""}${Math.round((category.gb / capacity.usedGb) * 100)}% of used space`}
                    </span>
                  </span>
                </div>
                <span className="st-blurb">{category.blurb}</span>
                {category.top.length > 0 && (
                  <ul className="v2-info-list">
                    {category.top.map((entry) => (
                      <li key={entry.name}><span className="v2-ellipsis">{entry.name}</span><strong className="v2-num">{formatGb(entry.gb)}</strong></li>
                    ))}
                  </ul>
                )}
                <div className="st-detail-foot">
                  <Pill status={category.pillStatus}>{category.pill}</Pill>
                  {category.browse && (
                    <Button variant="soft" size="sm" onClick={() => { setView("folders"); setPath(category.browse ?? []); }}>Show folders</Button>
                  )}
                  {category.cleanupMode && (
                    <Button variant="soft" size="sm" onClick={() => category.cleanupMode && props.onScanInCleanup(category.cleanupMode)}>Scan in Cleanup</Button>
                  )}
                </div>
              </section>
            )}

            <section aria-labelledby="st-managed" className="v2-card st-managed">
              <h2 id="st-managed" className="v2-card-title-sm">Space macOS manages</h2>
              <span className="v2-card-note st-managed-note">Shown for context. Kleari doesn't remove any of these.</span>
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
        </div>
      </div>

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
          <div className="v2-dialog-actions-end">
            <Button
              variant="secondary"
              onClick={() => {
                setAccessOpen(false);
                setAccessAsked(true);
                analyzeNow();
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

      <AnalysisDialog
        open={analysisOpen}
        analysis={analysis}
        steps={props.analysisSteps}
        volume={props.volume}
        usedGb={capacity?.usedGb ?? null}
        hasResult={tree !== null}
        onCancel={props.onCancelAnalysis}
        onClose={() => {
          setAnalysisOpen(false);
          if (!analysis.stopped && !analysis.running && !analysis.failed) {
            toast.show("Storage analysis updated. Nothing was changed.");
          }
        }}
      />
    </>
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
      {/* Keyed by folder, so opening one starts its list at the top. */}
      <ScrollList key={path.join("/")} label={`Folders in ${current.name}`} className="st-folders">
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
                <IconTile tone={child.tone} icon={child.icon} size={30} />
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
      </ScrollList>
    </>
  );
}

/** The only part of the page that scrolls, under the scroll-edge glass, as on Processes. */
function ScrollList({ label, className, children }: { label: string; className: string; children: ReactNode }) {
  const [scrolled, onScroll] = useScrolled();
  return (
    <ul tabIndex={0} aria-label={label} className={className} onScroll={onScroll}>
      <ScrollEdgeGlass visible={scrolled} inset={0} as="li" />
      {children}
    </ul>
  );
}

interface AnalysisDialogProps {
  open: boolean;
  analysis: StorageAnalysis;
  steps: readonly AnalysisStep[];
  volume: string;
  usedGb: number | null;
  hasResult: boolean;
  onCancel: () => void;
  onClose: () => void;
}

function AnalysisDialog({ open, analysis, steps, volume, usedGb, hasResult, onCancel, onClose }: AnalysisDialogProps) {
  const { running, stopped, failed, percent, stepIndex, countedGb } = analysis;
  const complete = !running && !stopped && !failed && hasResult;
  const ended = !running && !complete;
  const tone = ended ? "orange" : running ? "blue" : "green";
  // Without used space there is no byte denominator, so progress is finished steps only.
  const stepText = `Step ${Math.min(stepIndex + 1, steps.length)} of ${steps.length}`;
  const barPercent = percent ?? (stepIndex / Math.max(steps.length, 1)) * 100;
  const title = running ? `Analyzing ${volume}` : failed ? "Analysis didn’t finish" : stopped ? "Analysis stopped" : "Analysis complete";
  const text = running
    ? `${steps[stepIndex]?.label ?? "Reading"}…`
    : failed ?? (stopped ? (hasResult ? "Showing the results from your last analysis." : "Nothing was counted. Analyze again when you’re ready.") : analysis.doneNote ?? "");
  return (
    <Modal open={open} onClose={() => (running ? onCancel() : onClose())} labelledBy="st-an-title" className="st-an">
      <div className="v2-dialog-body st-an-body">
        <div className="v2-dialog-head">
          <IconTile tone={tone} icon={ended ? "warning" : running ? "disk" : "check"} size={48} glow />
          <div className="v2-row-text">
            <h2 id="st-an-title" className="v2-dialog-title">{title}</h2>
            <span aria-live="polite" className="v2-dialog-text">{text}</span>
          </div>
        </div>
        {!ended && (
          <>
            <div className="st-an-progress">
              <ProgressBar
                percent={Math.max(barPercent, 3)}
                label="Storage analysis"
                valueText={percent === null ? stepText : `${percent}%`}
                fill={running ? "scan" : "ok"}
                running={running}
              />
              <div className="st-an-numbers">
                <span>{usedGb === null ? `${formatGb(countedGb)} counted` : `${formatGb(countedGb)} of ${Math.round(usedGb)} GB used counted`}</span>
                <strong>{percent === null ? stepText : `${percent}%`}</strong>
              </div>
            </div>
            <ul className="v2-info-list st-an-steps">
              {steps.map((step, position) => {
                const isDone = complete || position < stepIndex;
                const isNow = running && position === stepIndex;
                return (
                  <li key={step.label} className={isDone || isNow ? "" : "wait"}>
                    <span className="st-an-icon" aria-hidden="true">
                      {isDone ? <StatusMark tone="ok" /> : isNow ? <Spinner size={12} /> : <span className="st-an-wait" />}
                    </span>
                    <span className={`st-an-label${isNow ? " now" : ""}`}>{step.label}</span>
                    <span className="v2-muted v2-num st-an-size">{isDone && step.gb !== null ? sizeText(step.gb, step.lowerBound) : isNow ? "Reading…" : ""}</span>
                  </li>
                );
              })}
            </ul>
          </>
        )}
        <div className="st-an-note">
          <Glyph name="shield" size={15} />
          <span>Read-only. Analyzing only measures sizes; nothing is moved or deleted.</span>
        </div>
        <div className="v2-dialog-actions-end">
          {running ? (
            <Button variant="secondary" onClick={onCancel}>Cancel</Button>
          ) : (
            <Button variant="primary" onClick={onClose}>{complete ? "Done" : "Close"}</Button>
          )}
        </div>
      </div>
    </Modal>
  );
}
