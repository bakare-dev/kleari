import { useEffect, useId, useRef, useState } from "react";
import { Glyph, StatusMark, type IconName } from "../../ui/icons";
import { Button, Dot, IconTile, LinkButton, Modal, PageHeader, Pill, ScrollBody, Spinner, Toast, radioGroupKeyDown, useToast, type Tone } from "../../ui/components";
import { formatGb, plural } from "../../ui/format";
import { headingIdFor, hrefFor, navigate } from "../../shell/routes";
import { CleanupSteps } from "./CleanupSteps";
import {
  type CleanupScanState,
  type FolderAccess,
  type LastScanSummary,
  type ProtectedFolder,
  type ScanMode,
  type ScanModeDefinition,
  type ScanOptions,
  type ScanPreset,
  type ScopeFolder,
  type StepFound,
} from "./types";
import "./cleanup.css";

/** Something the page should say about the last scan or the folders. */
export interface CleanupNotices {
  /** macOS refused some items; Full Disk Access would let Kleari read them. */
  fullDiskAccess: boolean;
  /** Granted or added folders that couldn't be searched. */
  skipped: readonly string[];
  /** Why the scan or the folder list couldn't load. */
  error: string | null;
}

interface CleanupScreenProps {
  modes: Record<ScanMode, ScanModeDefinition>;
  folders: readonly ScopeFolder[];
  /** `null` until a scan has found something. */
  lastScan: LastScanSummary | null;
  scan: CleanupScanState;
  notices: CleanupNotices;
  /** A mode to start from (Storage's Scan in Cleanup); it never starts a scan. */
  preset?: ScanPreset | null;
  /** The options menu's choices. */
  options: ScanOptions;
  onOptionsChange: (options: ScanOptions) => void;
  onStart: (mode: ScanMode, options: ScanOptions) => void;
  onCancel: () => void;
  onReset: (mode: ScanMode) => void;
  /** Opens the macOS folder panel; resolves to a message to show, if any. */
  onAddFolder: () => Promise<string | null>;
  /** Reads the folder so macOS asks; resolves to the answer. */
  onRequestAccess: (folder: ProtectedFolder) => Promise<FolderAccess | null>;
  onOpenFilesAndFolders: () => void;
  onOpenFullDiskAccess: () => void;
}

const modeOrder: readonly ScanMode[] = ["standard", "deep", "dev"];

const accessFacts: readonly { text: string; tone: "ok" | "info" }[] = [
  { text: "Reads names, sizes and dates. File contents never leave this Mac.", tone: "ok" },
  { text: "Nothing is moved or deleted without a preview you approve.", tone: "ok" },
  { text: "You can turn access off again in System Settings › Privacy & Security › Files and Folders.", tone: "info" },
];

function AccessFacts() {
  return (
    <ul className="v2-facts clean-access-facts">
      {accessFacts.map((fact) => (
        <li key={fact.text}>
          <span className={fact.tone === "ok" ? "clean-fact-ok" : "clean-fact-info"}>
            <Glyph name={fact.tone === "ok" ? "check" : "info"} size={14} strokeWidth={2.4} />
          </span>
          <span>{fact.text}</span>
        </li>
      ))}
    </ul>
  );
}

/** What the selected mode will look for, reflecting the options menu. */
function looksFor(mode: ScanMode, options: ScanOptions): { label: string; sub: string }[] {
  if (mode === "standard") {
    return [
      { label: "App caches", sub: "Apps download these again when they need them" },
      { label: "Package caches", sub: "Homebrew, npm, pip and other tools fetch these again" },
      ...(options.logs ? [{ label: "Logs and crash reports", sub: `Not written for ${options.logAge}` }] : []),
      ...(options.temp ? [{ label: "Temporary files", sub: "Left behind by installers and apps, untouched for a week" }] : []),
      { label: "Broken items", sub: "Damaged preferences files and login items whose program is gone" },
    ];
  }
  if (mode === "deep") {
    return [
      { label: "Everything in Standard", sub: "Caches, logs, temporary files and broken items" },
      { label: "Large files", sub: `Over ${options.size} in the folders you granted` },
      { label: "Old files", sub: `Not opened in ${options.age}${options.hidden ? ", hidden files included" : ""}` },
    ];
  }
  return [
    ...(options.xcode ? [{ label: "Xcode DerivedData, previews, archives and device support", sub: "Build output and symbols Xcode can make again" }] : []),
    ...(options.sim ? [{ label: "iOS Simulator", sub: "Caches to review, and devices listed for Xcode to manage" }] : []),
    ...(options.node ? [{ label: "node_modules", sub: "Checked against each project’s lockfile" }] : []),
    ...(options.docker ? [{ label: "Docker disk use", sub: "Listed only, never changed" }] : []),
  ];
}

type MenuEntry =
  | { kind: "label"; label: string }
  | { kind: "sep" }
  | { kind: "check"; label: string; key: "logs" | "temp" | "hidden" | "xcode" | "sim" | "node" | "docker" }
  | { kind: "radio"; label: string; key: "logAge" | "size" | "age"; value: string }
  | { kind: "start" }
  | { kind: "item"; label: string; hint?: string; disabled?: boolean; run?: () => void };

/** Scheduled scans are set in Settings › Scanning (ADR-015); this opens it there. */
const scheduleItem: MenuEntry = { kind: "item", label: "Schedule a scan…", hint: "In Settings", run: () => navigate("settings", "scanning") };

export function menuFor(mode: ScanMode, onAddFolder: () => void): MenuEntry[] {
  const radios = (key: "logAge" | "size" | "age", values: string[]): MenuEntry[] => values.map((value) => ({ kind: "radio", label: value, key, value }));
  if (mode === "standard") {
    return [
      { kind: "start" }, { kind: "sep" }, { kind: "label", label: "Include" },
      { kind: "check", label: "Logs and crash reports", key: "logs" }, { kind: "check", label: "Temporary files", key: "temp" },
      { kind: "sep" }, { kind: "label", label: "Only logs older than" }, ...radios("logAge", ["7 days", "30 days", "90 days"]),
      { kind: "sep" }, scheduleItem,
    ];
  }
  if (mode === "deep") {
    return [
      { kind: "start" }, { kind: "sep" }, { kind: "label", label: "Large files over" }, ...radios("size", ["500 MB", "1 GB", "5 GB"]),
      { kind: "label", label: "Old files not opened in" }, ...radios("age", ["3 months", "6 months", "1 year"]),
      { kind: "sep" }, { kind: "check", label: "Include hidden files", key: "hidden" },
      { kind: "item", label: "Add a folder…", run: onAddFolder },
      scheduleItem,
    ];
  }
  return [
    { kind: "start" }, { kind: "sep" }, { kind: "label", label: "Tools to check" },
    { kind: "check", label: "Xcode DerivedData and archives", key: "xcode" }, { kind: "check", label: "iOS Simulator", key: "sim" },
    { kind: "check", label: "node_modules", key: "node" }, { kind: "check", label: "Docker disk use (read-only)", key: "docker" },
    { kind: "sep" }, { kind: "item", label: "Add a project folder…", run: onAddFolder },
  ];
}

interface RunMenuProps {
  mode: ScanModeDefinition;
  modeId: ScanMode;
  options: ScanOptions;
  setOptions: (options: ScanOptions) => void;
  onStart: () => void;
  onAddFolder: () => void;
  disabled: boolean;
}

function RunButton({ mode, modeId, options, setOptions, onStart, onAddFolder, disabled }: RunMenuProps) {
  const [open, setOpen] = useState(false);
  const menuId = useId();
  const root = useRef<HTMLDivElement>(null);
  const menu = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    menu.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    function onPointer(event: PointerEvent) {
      if (root.current && event.target instanceof Node && !root.current.contains(event.target)) {
        setOpen(false);
      }
    }
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpen(false);
      }
    }
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div className={`clean-split tone-${mode.tone}`} ref={root}>
      <div className="clean-split-buttons">
        <button type="button" className="clean-split-main" disabled={disabled} onClick={onStart}>Start {mode.name}</button>
        <button
          type="button"
          className="clean-split-more"
          aria-label={`${mode.title} options`}
          aria-haspopup="menu"
          aria-expanded={open}
          aria-controls={menuId}
          onClick={() => setOpen((current) => !current)}
        >
          <span style={{ transform: open ? "rotate(180deg)" : "none", display: "flex" }}><Glyph name="chevronDown" size={12} strokeWidth={2.6} /></span>
        </button>
      </div>
      {open && (
        <div id={menuId} ref={menu} className="v2-menu clean-menu" role="menu" aria-label={`${mode.title} options`}>
          {menuFor(modeId, () => { setOpen(false); onAddFolder(); }).map((entry, index) => {
            switch (entry.kind) {
              case "label":
                return <div key={index} className="v2-menu-label">{entry.label}</div>;
              case "sep":
                return <div key={index} className="v2-menu-sep" role="separator" />;
              case "start":
                return (
                  <button key={index} type="button" role="menuitem" className="v2-menu-item clean-menu-start" disabled={disabled} onClick={() => { setOpen(false); onStart(); }}>
                    <span className="v2-menu-mark" aria-hidden="true" />
                    <span>Start {mode.name}</span>
                    <span className="v2-menu-hint">↩</span>
                  </button>
                );
              case "check": {
                const on = options[entry.key];
                return (
                  <button key={index} type="button" role="menuitemcheckbox" aria-checked={on} className="v2-menu-item" onClick={() => setOptions({ ...options, [entry.key]: !on })}>
                    <span className="v2-menu-mark clean-menu-mark" aria-hidden="true">{on ? "✓" : ""}</span>
                    <span>{entry.label}</span>
                  </button>
                );
              }
              case "radio": {
                const on = options[entry.key] === entry.value;
                return (
                  <button key={index} type="button" role="menuitemradio" aria-checked={on} className="v2-menu-item" onClick={() => setOptions({ ...options, [entry.key]: entry.value })}>
                    <span className="v2-menu-mark clean-menu-mark" aria-hidden="true">{on ? "•" : ""}</span>
                    <span>{entry.label}</span>
                  </button>
                );
              }
              case "item":
                return (
                  <button key={index} type="button" role="menuitem" className="v2-menu-item" disabled={entry.disabled} onClick={entry.run}>
                    <span className="v2-menu-mark" aria-hidden="true" />
                    <span>{entry.label}</span>
                    {entry.hint && <span className="v2-menu-hint">{entry.hint}</span>}
                  </button>
                );
            }
          })}
        </div>
      )}
    </div>
  );
}

function RunRing({ phase, done, total, tone, modeIcon }: { phase: CleanupScanState["phase"]; done: number; total: number; tone: Tone; modeIcon: IconName }) {
  const circumference = 2 * Math.PI * 40;
  const ringTone: Tone = phase === "cancelled" ? "slate" : phase === "done" ? "green" : tone;
  const heroIcon: IconName = phase === "cancelled" ? "close" : phase === "done" ? "check" : modeIcon;
  const dash = phase === "cancelled" ? `${((circumference * done) / Math.max(total, 1)).toFixed(1)} ${circumference.toFixed(1)}` : `${circumference.toFixed(1)} 0`;
  return (
    <div className={`clean-ring tone-${tone}`}>
      <svg aria-hidden="true" width="92" height="92" viewBox="0 0 92 92">
        <defs>
          <linearGradient id="clean-ring" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" className="v2-stop-1" />
            <stop offset="1" className="v2-stop-2" />
          </linearGradient>
        </defs>
        <circle cx="46" cy="46" r="40" fill="none" className="v2-track-stroke" strokeWidth="7" />
        {phase !== "scanning" && (
          <circle cx="46" cy="46" r="40" fill="none" stroke="url(#clean-ring)" strokeWidth="7" strokeLinecap="round" strokeDasharray={dash} transform="rotate(-90 46 46)" />
        )}
      </svg>
      {phase === "scanning" && (
        <svg className="v2-spin clean-ring-spin" aria-hidden="true" width="92" height="92" viewBox="0 0 92 92">
          <circle cx="46" cy="46" r="40" fill="none" stroke="url(#clean-ring)" strokeWidth="7" strokeLinecap="round" strokeDasharray="70 182" />
        </svg>
      )}
      <span className={`clean-ring-hero tone-${ringTone}`}>
        <Glyph name={heroIcon} size={28} strokeWidth={2.1} />
      </span>
    </div>
  );
}

function FolderAction({ folder, onGrant }: { folder: ScopeFolder; onGrant: () => void }) {
  switch (folder.access) {
    case "available":
    case "granted":
      return <Pill status="ok">✓ {folder.access === "granted" ? "Granted" : "Readable"}</Pill>;
    case "fullDiskAccess":
      return <Pill status="ok">✓ Full Disk Access</Pill>;
    case "missing":
      return <Pill status="muted">Not on this Mac</Pill>;
    case "notRequested":
    case "denied":
      return <Button variant="soft" size="sm" className="clean-grant" onClick={onGrant}>Grant access…</Button>;
  }
}

type GrantStep = "explain" | "waiting" | "denied";

/** "1:05" since `startedAtMs`, ticking every second while `running`. */
function useElapsed(startedAtMs: number | null | undefined, running: boolean): string | null {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running) {
      return;
    }
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [running]);
  if (!running || startedAtMs === null || startedAtMs === undefined) {
    return null;
  }
  const seconds = Math.max(0, Math.floor((now - startedAtMs) / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

function foundText(found: StepFound | undefined): string {
  return found ? `${plural(found.items, "item")} · ${formatGb(found.bytes / 1e9)}` : "";
}

export function CleanupScreen(props: CleanupScreenProps) {
  const { modes, folders, lastScan, scan, notices, options } = props;
  const [selectedMode, setSelectedMode] = useState<ScanMode>(props.preset?.mode ?? scan.mode);
  const [grant, setGrant] = useState<{ folder: ScopeFolder; step: GrantStep } | null>(null);
  const toast = useToast();

  // While a scan runs or its results are shown, the card describes that scan's mode.
  const modeId: ScanMode = scan.phase === "idle" ? selectedMode : scan.mode;
  const mode = modes[modeId];
  const phase = scan.phase;
  const total = mode.steps.length;
  const step = phase === "done" ? total : Math.min(scan.step, total);
  const doneSteps = phase === "done" ? total : Math.max(step - 1, 0);
  const elapsed = useElapsed(scan.startedAtMs, phase === "scanning");
  const found = scan.found ?? [];
  const foundSoFar = found.reduce((sum, entry) => ({ items: sum.items + (entry?.items ?? 0), bytes: sum.bytes + (entry?.bytes ?? 0) }), { items: 0, bytes: 0 });

  const titles: Record<CleanupScanState["phase"], [string, string]> = {
    idle: [`Ready for a ${mode.name}`, mode.idleDetail],
    scanning: [
      `${mode.steps[Math.max(step - 1, 0)]?.label ?? "Reading"}…`,
      `Step ${Math.max(step, 1)} of ${total} · ${foundSoFar.items ? `${foundText(foundSoFar)} found so far` : "looking"}${elapsed ? ` · ${elapsed}` : ""}. Reading file details only; you can cancel at any time.`,
    ],
    cancelled: ["Scan cancelled", `Stopped during step ${Math.max(step, 1)} of ${total}. Partial results were kept and marked incomplete. Nothing was changed.`],
    done: [`${mode.title} complete`, mode.doneDetail],
  };

  const chooseMode = (next: ScanMode) => {
    setSelectedMode(next);
    if (scan.phase !== "idle" && scan.phase !== "scanning") {
      props.onReset(next);
    }
  };

  const addFolder = () => {
    void props.onAddFolder().then((message) => message && toast.show(message));
  };

  const requestAccess = (folder: ScopeFolder) => {
    if (!folder.protectedFolder) {
      return;
    }
    setGrant({ folder, step: "waiting" });
    void props.onRequestAccess(folder.protectedFolder).then((access) => {
      if (access === "granted") {
        setGrant(null);
        toast.show(`Kleari can now read ${folder.path}`);
      } else if (access === "denied") {
        setGrant({ folder, step: "denied" });
      } else {
        setGrant(null);
        if (access === "missing") {
          toast.show(`${folder.path} isn’t on this Mac`, "warn");
        }
      }
    });
  };

  const looks = looksFor(modeId, options);
  const scanning = phase === "scanning";

  return (
    <>
      <PageHeader
        above={<CleanupSteps current={0} />}
        title="Cleanup"
        headingId={headingIdFor("cleanup")}
        subtitle="Scans only read. Nothing moves until you approve a preview."
      />
      <ScrollBody label="Cleanup" className="clean-body" edgeGlass={false}>
        {notices.error && (
          <div role="alert" className="v2-banner v2-banner-warn">
            <IconTile tone="orange" icon="warning" size={32} />
            <div className="v2-banner-text">
              <strong>{notices.error}</strong>
            </div>
          </div>
        )}
        {notices.fullDiskAccess && (
          <div role="status" className="v2-banner v2-banner-warn">
            <IconTile tone="orange" icon="lock" size={32} />
            <div className="v2-banner-text">
              <strong>macOS kept some folders private</strong>
              <span>Allow Full Disk Access for Kleari in System Settings, then scan again to include them. Nothing else changes.</span>
            </div>
            <Button variant="soft" size="sm" onClick={props.onOpenFullDiskAccess}>Open System Settings</Button>
          </div>
        )}

        <section aria-label="Scan type" role="radiogroup" className="v2-grid-3" onKeyDown={(event) => radioGroupKeyDown(event, modeOrder, modeId, chooseMode)}>
          {modeOrder.map((id) => {
            const definition = modes[id];
            const on = id === modeId;
            return (
              <button
                key={id}
                type="button"
                role="radio"
                aria-checked={on}
                tabIndex={on ? 0 : -1}
                disabled={scanning && !on}
                className={`v2-card clean-mode${on ? " on" : ""}`}
                onClick={() => chooseMode(id)}
              >
                <IconTile tone={definition.tone} icon={definition.icon} size={40} glow />
                <span className="clean-mode-text">
                  <span className="clean-mode-title">
                    <span>{definition.title}</span>
                    {on && <StatusMark tone="accent" size={18} />}
                  </span>
                  <span className="clean-mode-sub">{definition.sub}</span>
                  <span className="clean-mode-meta">{definition.meta}</span>
                </span>
              </button>
            );
          })}
        </section>

        <div className="clean-pair">
          <section aria-labelledby="clean-scope" className="v2-card clean-scope">
            <div className="v2-card-head">
              <div>
                <h2 id="clean-scope" className="v2-card-title">Folders to scan</h2>
                <span className="v2-card-note">Kleari only reads folders it’s allowed to.</span>
              </div>
              <Button variant="soft" size="sm" disabled={scanning} onClick={addFolder}>+ Add folder</Button>
            </div>
            {folders.map((folder) => {
              const used = folder.modes.includes(modeId);
              return (
                <div key={folder.key} className="clean-folder" style={{ opacity: used ? 1 : 0.5 }}>
                  <IconTile tone={folder.tone} icon="folder" size={32} />
                  <span className="v2-row-text">
                    <span className="v2-mono clean-folder-path">{folder.path}</span>
                    <span className="clean-folder-note">{used ? folder.note : `Not part of the ${mode.name}`}</span>
                  </span>
                  <FolderAction folder={folder} onGrant={() => setGrant({ folder, step: "explain" })} />
                </div>
              );
            })}
          </section>

          <section aria-labelledby="clean-run" className={`v2-card clean-run tone-${mode.tone}`}>
            <div className="clean-run-head">
              <RunRing phase={phase} done={doneSteps} total={total} tone={mode.tone} modeIcon={mode.icon} />
              <div role="status" className="clean-run-status">
                <span className="clean-run-mode">{mode.title}</span>
                <h2 id="clean-run" className="clean-run-title">{titles[phase][0]}</h2>
                <span className="clean-run-detail">{titles[phase][1]}</span>
              </div>
            </div>

            {phase === "idle" && (
              <div className="clean-looks">
                <span className="clean-looks-label">This scan looks for</span>
                <ul>
                  {looks.map((look) => (
                    <li key={look.label}>
                      <span className="clean-look-tick" aria-hidden="true"><Glyph name="check" size={12} strokeWidth={3} /></span>
                      <span className="v2-row-text">
                        <span className="clean-look-title">{look.label}</span>
                        <span className="clean-look-sub">{look.sub}</span>
                      </span>
                    </li>
                  ))}
                </ul>
                {mode.note && (
                  <div className="clean-note">
                    <Glyph name="info" size={14} strokeWidth={2.2} />
                    <span>{mode.note}</span>
                  </div>
                )}
              </div>
            )}

            {(phase === "scanning" || phase === "cancelled") && (
              <div className="clean-progress">
                <ol>
                  {mode.steps.map((definition, index) => {
                    const isDone = index < step - 1;
                    const isActive = phase === "scanning" && index === step - 1;
                    const stopped = phase === "cancelled" && index === step - 1;
                    return (
                      <li key={definition.label} className={isDone || isActive ? "" : "pending"}>
                        {isDone ? <StatusMark tone="ok" size={18} /> : isActive ? <Spinner size={16} tone="accent" /> : <span className="clean-step-wait" aria-hidden="true" />}
                        <span className={isActive ? "clean-step-active" : ""}>
                          {definition.label}
                          {isActive ? "…" : stopped ? " (stopped)" : ""}
                        </span>
                        <span className="clean-step-meta">
                          {isDone
                            ? definition.found || foundText(found[index])
                            : isActive
                              ? found[index]?.items
                                ? `${plural(found[index].items, "item")} found`
                                : "reading…"
                              : ""}
                        </span>
                      </li>
                    );
                  })}
                </ol>
                {/* The step in progress counts as half done, so the bar moves during a step. */}
                <div
                  className={`clean-bar${phase === "scanning" ? " running" : ""}`}
                  role="progressbar"
                  aria-label="Scan steps completed"
                  aria-valuemin={0}
                  aria-valuemax={total}
                  aria-valuenow={doneSteps}
                  aria-valuetext={`${doneSteps} of ${total} steps done`}
                >
                  <div style={{ width: `${Math.round(((doneSteps + (phase === "scanning" ? 0.5 : 0)) / Math.max(total, 1)) * 100)}%` }} />
                </div>
                <span className="clean-progress-text" aria-live="polite">
                  {phase === "cancelled"
                    ? `${doneSteps} of ${total} steps finished before you cancelled`
                    : `${doneSteps} of ${total} steps done${foundSoFar.items ? ` · ${foundText(foundSoFar)} found` : ""}${elapsed ? ` · ${elapsed} elapsed` : ""}`}
                </span>
              </div>
            )}

            {phase === "done" && (
              <ul className="clean-results">
                {mode.results.map((result) => (
                  <li key={result.label}>
                    <Dot tone={result.tone} />
                    <span className="clean-result-label">{result.label} <span>· {result.count}</span></span>
                    <span className="clean-result-size">{result.size}</span>
                    <Pill status={result.status}>{result.tag}</Pill>
                  </li>
                ))}
                {notices.skipped.map((folder) => (
                  <li key={folder}>
                    <Dot tone="slate" />
                    <span className="clean-result-label">{folder} <span>· skipped</span></span>
                    <span className="clean-result-size" />
                    <Pill status="warn">Couldn’t read</Pill>
                  </li>
                ))}
              </ul>
            )}

            <div className="clean-run-foot">
              <span className="v2-card-note">
                {phase === "idle" ? mode.foot : phase === "scanning" ? "Nothing will move without your approval" : "Results stay until you scan again"}
              </span>
              {phase === "idle" && (
                <RunButton
                  mode={mode}
                  modeId={modeId}
                  options={options}
                  setOptions={props.onOptionsChange}
                  onStart={() => props.onStart(modeId, options)}
                  onAddFolder={addFolder}
                  disabled={mode.steps.length === 0}
                />
              )}
              {phase === "scanning" && <Button variant="secondary" onClick={props.onCancel}>Cancel scan</Button>}
              {(phase === "done" || phase === "cancelled") && (
                <div className="clean-run-actions">
                  <Button variant="secondary" className="clean-again" onClick={() => props.onReset(modeId)}>Scan again</Button>
                  <LinkButton href={hrefFor("review")} variant="primary" className={`clean-review tone-${mode.tone}`}>Review results →</LinkButton>
                </div>
              )}
            </div>
          </section>
        </div>

        {lastScan && (
          <section aria-labelledby="clean-last" className="v2-card v2-card-pad clean-last">
            <div className="v2-card-head clean-last-head">
              <div>
                <h2 id="clean-last" className="v2-card-title">{lastScan.title}</h2>
                <span className="v2-card-note">{lastScan.sub}</span>
              </div>
              <LinkButton href={hrefFor("review")} size="sm">See all results →</LinkButton>
            </div>
            <div
              className="clean-last-bar"
              role="img"
              aria-label={`Found space by category: ${lastScan.categories.map((category) => `${category.name} ${category.gb} GB`).join(", ")}`}
            >
              {lastScan.categories.map((category) => (
                <span key={category.name} className={`tone-${category.tone}`} style={{ width: `${(lastScan.totalGb > 0 ? (category.gb / lastScan.totalGb) * 100 : 0).toFixed(1)}%` }} />
              ))}
            </div>
            <div className="v2-grid-4">
              {lastScan.categories.map((category) => (
                <div key={category.name} className="clean-last-cat">
                  <span className="clean-last-name"><Dot tone={category.tone} />{category.name}</span>
                  <span className="clean-last-size">{category.gb} GB <span>· {category.items} items</span></span>
                  <span className="clean-last-pills">
                    <Pill status={category.riskStatus}>{category.risk}</Pill>
                    <Pill status={category.eligibilityStatus}>{category.eligibility}</Pill>
                  </span>
                </div>
              ))}
            </div>
          </section>
        )}
      </ScrollBody>

      <Toast message={toast.message} bottom={22} />

      <Modal open={grant !== null} onClose={() => grant?.step !== "waiting" && setGrant(null)} labelledBy="clean-grant-title">
        {grant && (
          <div className="v2-dialog-body v2-dialog-centered">
            <IconTile tone={grant.step === "denied" ? "orange" : "green"} icon={grant.step === "denied" ? "lock" : "folderGrant"} size={60} glow />
            {grant.step === "waiting" ? (
              <>
                <h2 id="clean-grant-title" className="v2-dialog-title">Waiting for macOS…</h2>
                <p role="status" className="v2-dialog-text">
                  Confirm access to {grant.folder.path} in the macOS window. If you decline there, the folder stays skipped and nothing else changes.
                </p>
                <span className="clean-step-spin clean-grant-spin" aria-hidden="true" />
              </>
            ) : grant.step === "denied" ? (
              <>
                <h2 id="clean-grant-title" className="v2-dialog-title">macOS didn’t allow access to {grant.folder.path}</h2>
                <p className="v2-dialog-text">
                  You can allow it in System Settings › Privacy & Security › Files and Folders by turning on {grant.folder.path.replace("~/", "")} for Kleari. Then grant access again here.
                </p>
                <div className="v2-dialog-actions">
                  <Button variant="secondary" onClick={() => setGrant(null)}>Not now</Button>
                  <Button variant="primary" onClick={() => { setGrant(null); props.onOpenFilesAndFolders(); }}>Open System Settings</Button>
                </div>
              </>
            ) : (
              <>
                <h2 id="clean-grant-title" className="v2-dialog-title">Let Kleari read {grant.folder.path}?</h2>
                <p className="v2-dialog-text">
                  Deep scans use this folder to find large and old files for you to inspect. Kleari never moves them. You can say no and keep scanning everything else.
                </p>
                <AccessFacts />
                <span className="v2-card-note">Next, macOS will ask you to confirm in its own window.</span>
                <div className="v2-dialog-actions">
                  <Button variant="secondary" onClick={() => setGrant(null)}>Not now</Button>
                  <Button variant="primary" onClick={() => requestAccess(grant.folder)}>Continue</Button>
                </div>
              </>
            )}
          </div>
        )}
      </Modal>
    </>
  );
}
