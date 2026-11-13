import type { IconName } from "../../ui/icons";
import type { Status, Tone } from "../../ui/components";

export type FindingCategory = "cache" | "xcode" | "pkg" | "node" | "build" | "logs" | "large" | "old" | "apps";
export type Eligibility = "eligible" | "notNow" | "needsCheck" | "rescan" | "inspectOnly";
export type Level = "Low" | "Medium" | "Unknown";

export interface Evidence {
  tone: "ok" | "warn" | "info";
  text: string;
}

export interface Finding {
  id: string;
  name: string;
  category: FindingCategory;
  path: string;
  gb: number;
  files: string;
  modified: string;
  reason: string;
  certainty: "High" | "Medium" | "Unknown";
  risk: Level;
  eligibility: Eligibility;
  eligibilityLong: string;
  /** Why it can't be selected; set for every non-eligible finding. */
  whyNot?: string;
  evidence: readonly Evidence[];
}

/** Only eligible findings can be selected for a preview. */
export function isSelectable(finding: Finding): boolean {
  return finding.eligibility === "eligible";
}

export type ScanMode = "standard" | "deep" | "dev";
export type ScanPhase = "idle" | "scanning" | "cancelled" | "done";

export interface ScanStepDefinition {
  label: string;
  /** What the step found, shown once it is done. */
  found: string;
}

export interface ScanResultLine {
  label: string;
  count: string;
  size: string;
  tag: string;
  status: Status;
  tone: Tone;
}

export interface ScanModeDefinition {
  title: string;
  name: string;
  sub: string;
  meta: string;
  tone: Tone;
  icon: IconName;
  note: string;
  idleDetail: string;
  foot: string;
  steps: readonly ScanStepDefinition[];
  results: readonly ScanResultLine[];
  doneDetail: string;
}

/** Whether Kleari may read a folder: readable, granted by you, covered by Full Disk
 * Access, not asked yet, or refused. */
export type FolderAccess = "available" | "granted" | "fullDiskAccess" | "notRequested" | "denied" | "missing";

/** A folder macOS protects, which Kleari reads only after you grant access. */
export type ProtectedFolder = "downloads" | "desktop" | "documents" | "movies";

export interface ScopeFolder {
  key: string;
  path: string;
  note: string;
  access: FolderAccess;
  tone: Tone;
  /** The scan modes that read it. */
  modes: readonly ScanMode[];
  /** Set for folders macOS protects, so access can be requested. */
  protectedFolder?: ProtectedFolder;
  /** Added by the user from the folder picker. */
  added?: boolean;
}

export interface LastScanCategory {
  name: string;
  gb: number;
  items: number;
  tone: Tone;
  risk: string;
  riskStatus: Status;
  eligibility: string;
  eligibilityStatus: Status;
}

export interface LastScanSummary {
  title: string;
  sub: string;
  totalGb: number;
  categories: readonly LastScanCategory[];
}

export interface ScanOptions {
  logs: boolean;
  temp: boolean;
  logAge: "7 days" | "30 days" | "90 days";
  size: "500 MB" | "1 GB" | "5 GB";
  age: "3 months" | "6 months" | "1 year";
  hidden: boolean;
  xcode: boolean;
  sim: boolean;
  node: boolean;
  docker: boolean;
}

export interface CleanupScanState {
  mode: ScanMode;
  phase: ScanPhase;
  /** 1-based index of the step in progress (or where it stopped). */
  step: number;
  /** What each step has found so far, by step index. */
  found?: readonly StepFound[];
  /** When the running scan started, for the elapsed time. */
  startedAtMs?: number | null;
}

export interface StepFound {
  items: number;
  bytes: number;
}

/** A mode and options to start from, such as Storage's Scan in Cleanup. Never starts a scan. */
export interface ScanPreset {
  mode: ScanMode;
  options?: Partial<ScanOptions>;
}

export const defaultScanOptions: ScanOptions = {
  logs: true,
  temp: true,
  logAge: "30 days",
  size: "1 GB",
  age: "6 months",
  hidden: false,
  xcode: true,
  sim: true,
  node: true,
  docker: true,
};
