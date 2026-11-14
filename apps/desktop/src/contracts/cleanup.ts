// IPC contracts for Cleanup, Review, Preview and Quarantine. Mirrors src-tauri/src/cleanup/
// and src-tauri/src/quarantine/; every response is validated before the UI sees it. Paths
// never arrive here: only opaque IDs, names and `~/…` locations.

import { parseDockerState, type DockerState } from "./developer";
import { fileCategories, type FileCategory } from "./storage";
import { ContractError, nonNegativeInteger, nullable, oneOf, record } from "./systemMetrics";

export const scanModes = ["standard", "deep", "dev"] as const;
export type ScanMode = (typeof scanModes)[number];

export const findingCategories = ["cache", "xcode", "pkg", "node", "build", "logs", "large", "old", "apps", "chosen", "broken", "startup"] as const;
export type FindingCategory = (typeof findingCategories)[number];

const risks = ["low", "medium", "unknown"] as const;
export type Risk = (typeof risks)[number];

const certainties = ["high", "medium", "unknown"] as const;
export type Certainty = (typeof certainties)[number];

const eligibilities = ["eligible", "notNow", "needsCheck", "rescan", "inspectOnly"] as const;
export type Eligibility = (typeof eligibilities)[number];

const whyNots = [
  "appRunning",
  "xcodeRunning",
  "simulatorRunning",
  "noLockfile",
  "activeProject",
  "appleCache",
  "manageInXcode",
  "userFile",
  "openInApp",
  "otherVolume",
  "changedSinceScan",
] as const;
export type WhyNot = (typeof whyNots)[number];

export const evidenceCodes = [
  "inCaches",
  "appNotRunning",
  "appRunning",
  "noAppMatch",
  "appleCache",
  "packageCache",
  "refetchedOnInstall",
  "inDerivedData",
  "xcodeNotRunning",
  "xcodeRunning",
  "rebuildSlower",
  "archive",
  "losesDebugSymbols",
  "deviceSupport",
  "copiedAgainOnConnect",
  "simulatorCache",
  "simulatorRunning",
  "simulatorData",
  "sharedStore",
  "toolchain",
  "logOlderThan",
  "tempOlderThan",
  "notWrittenSince",
  "lockfileFound",
  "noLockfile",
  "projectActive",
  "projectInactive",
  "buildOutput",
  "reinstallNeeded",
  "largerThan",
  "notOpenedFor",
  "notKnownCache",
  "otherVolume",
  "leftoverOfRemovedApp",
  "leftoverNoApp",
  "losesAppData",
  "installerFile",
  "downloadAgain",
  "chosenByYou",
  "openInApp",
  "nothingOpen",
  "preferencesUnreadable",
  "appMakesNewPreferences",
  "programMissing",
  "agentNotStarted",
  "inXcodePreviews",
  "startsAtLogin",
  "stopsAtNextLogin",
] as const;
export type EvidenceCode = (typeof evidenceCodes)[number];

export interface EvidenceEntry {
  tone: "ok" | "warn" | "info";
  code: EvidenceCode;
  /** Days, or bytes for a size threshold, when the wording needs one. */
  value: number | null;
  app: string | null;
}

export type FindingSource = "scan" | "storage" | "developer" | "apps";

export interface ReviewFinding {
  id: string;
  name: string;
  category: FindingCategory;
  /** The folder it is in, shown as `~/…`. */
  location: string;
  /** The item itself, shown as `~/…`. */
  path: string;
  bytes: number;
  files: number;
  lowerBound: boolean;
  modifiedMs: number | null;
  risk: Risk;
  certainty: Certainty;
  eligibility: Eligibility;
  whyNot: WhyNot | null;
  evidence: EvidenceEntry[];
  source: FindingSource;
  /** A Developer catalog ID for a known tool cache. */
  tool: string | null;
  /** What kind of file a large or old file is, by its extension. */
  fileCategory: FileCategory | null;
}

export interface ReviewScan {
  mode: ScanMode;
  startedAtMs: number;
  cancelled: boolean;
}

export interface ReviewList {
  scan: ReviewScan | null;
  findings: ReviewFinding[];
}

export interface ScanOptionsInput {
  logs: boolean;
  temp: boolean;
  logAgeDays: 7 | 30 | 90;
  largeMinMb: 500 | 1000 | 5000;
  oldDays: 90 | 180 | 365;
  hidden: boolean;
  xcode: boolean;
  sim: boolean;
  node: boolean;
  docker: boolean;
}

export const stepIds = ["appCaches", "logsAndTemp", "packageCaches", "largeAndOld", "brokenItems", "xcode", "simulators", "nodeModules", "docker"] as const;
export type StepId = (typeof stepIds)[number];

export interface StepResult {
  id: StepId;
  items: number;
  bytes: number;
  finished: boolean;
}

export interface CleanupScanResult {
  mode: ScanMode;
  cancelled: boolean;
  startedAtMs: number;
  durationMs: number;
  steps: StepResult[];
  needsFullDiskAccess: boolean;
  skippedFolders: string[];
  docker: DockerState | null;
}

export interface CleanupProgress {
  step: number;
  /** What the step has found so far. */
  items: number;
  bytes: number;
}

export const folderIds = ["caches", "logs", "developer", "projects", "downloads", "desktop", "documents", "movies", "added"] as const;
export type FolderId = (typeof folderIds)[number];
export type ProtectedFolder = "downloads" | "desktop" | "documents" | "movies";

const accesses = ["available", "missing", "granted", "fullDiskAccess", "notRequested", "denied"] as const;
export type FolderAccess = (typeof accesses)[number];

export interface ScopeFolder {
  id: FolderId;
  path: string;
  access: FolderAccess;
  modes: ScanMode[];
}

export type AddedCleanupFolder = { status: "added"; path: string } | { status: "cancelled" };

export type PlannedState = "planned" | "changed" | "notEligible";

export interface PlannedItem {
  id: string;
  state: PlannedState;
  bytes: number;
}

export interface CleanupPreview {
  id: string;
  preparedAtMs: number;
  items: PlannedItem[];
}

const moveFailures = [
  "needsAccess",
  "missing",
  "changed",
  "otherVolume",
  "exists",
  "folderUnavailable",
  "quarantineUnavailable",
  "failed",
] as const;
export type MoveFailure = (typeof moveFailures)[number];

/** `unconfirmed`: the move happened but couldn't be checked; it is in Quarantine and
 * confirmed at the next launch, never reported as left in place. A finished approval's
 * `moved` items carry `heldId`, the quarantine item each became. */
export type MoveProgress =
  | { id: string; state: "checking" }
  | { id: string; state: "moved"; heldId?: string }
  | { id: string; state: "unconfirmed" }
  | { id: string; state: "failed"; reason: MoveFailure };

export const historyReasons = ["cannotOpen", "corrupt", "newerVersion", "migrationFailed", "writeFailed"] as const;
export type HistoryUnavailableReason = (typeof historyReasons)[number];

export type ApproveOutcome =
  | { status: "noPreview" }
  | { status: "stale"; changed: string[] }
  | { status: "historyUnavailable"; reason: HistoryUnavailableReason }
  | { status: "finished"; batchId: string; finishedAtMs: number; items: MoveProgress[] };

const itemStatuses = ["restorable", "conflict", "folderMissing", "missing", "pending", "unverified", "needsFolder"] as const;
export type QuarantineItemStatus = (typeof itemStatuses)[number];

export interface QuarantineItemView {
  id: string;
  name: string;
  category: FindingCategory;
  from: string;
  bytes: number;
  status: QuarantineItemStatus;
  /** Choosing its folder would allow a restore (`needsFolder`) or settle an interrupted one (`pending`). */
  chooseFolder: boolean;
  /** The folder the history records it was added from, as `~/…`. A hint only; absent when too broad. */
  neededFolder?: string;
}

export interface QuarantineBatchView {
  id: string;
  createdMs: number;
  origin: "standard" | "deep" | "dev" | "sent";
  items: QuarantineItemView[];
}

export type QuarantineList =
  | { status: "available"; folder: string; batches: QuarantineBatchView[] }
  | { status: "unavailable"; reason: HistoryUnavailableReason; folder: string };

const restoreOutcomes = ["restored", "conflict", "folderMissing", "missing", "needsAccess", "unconfirmed", "notAllowed", "needsFolder", "failed"] as const;
export type RestoreOutcomeKind = (typeof restoreOutcomes)[number];

export interface RestoreResult {
  id: string;
  outcome: RestoreOutcomeKind;
}

export interface DeleteProgress {
  id: string;
  state: "deleting" | "deleted" | "failed";
}

export interface DeleteSummary {
  deleted: string[];
  failed: string[];
  kept: string[];
  stopped: boolean;
}

/** Bounds above the backend's own. */
const MAX_FINDINGS = 20_000;
const MAX_TEXT = 4096;
const MAX_EVIDENCE = 12;

function text(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_TEXT) {
    throw new ContractError(field);
  }
  return value;
}

/** An opaque ID from the backend: hex digits only. */
function id(value: unknown, field: string): string {
  if (typeof value !== "string" || !/^[0-9a-f]{1,64}$/.test(value)) {
    throw new ContractError(field);
  }
  return value;
}

function boolean(value: unknown, field: string): boolean {
  if (typeof value !== "boolean") {
    throw new ContractError(field);
  }
  return value;
}

function list(value: unknown, field: string, max: number): unknown[] {
  if (!Array.isArray(value) || value.length > max) {
    throw new ContractError(field);
  }
  return value;
}

function ids(value: unknown, field: string): string[] {
  return list(value, field, MAX_FINDINGS).map((entry, index) => id(entry, `${field}[${index}]`));
}

function evidence(value: unknown, field: string): EvidenceEntry {
  const fields = record(value, field);
  return {
    tone: oneOf(["ok", "warn", "info"] as const, fields.tone, `${field}.tone`),
    code: oneOf(evidenceCodes, fields.code, `${field}.code`),
    value: fields.value === undefined ? null : nonNegativeInteger(fields.value, `${field}.value`),
    app: fields.app === undefined ? null : text(fields.app, `${field}.app`),
  };
}

function finding(value: unknown, field: string): ReviewFinding {
  const fields = record(value, field);
  const eligibility = oneOf(eligibilities, fields.eligibility, `${field}.eligibility`);
  const whyNot = nullable(fields.whyNot, (reason) => oneOf(whyNots, reason, `${field}.whyNot`));
  // Every item that can't be selected says why; an eligible one never carries a reason.
  if ((eligibility === "eligible") !== (whyNot === null)) {
    throw new ContractError(`${field}.whyNot`);
  }
  return {
    id: id(fields.id, `${field}.id`),
    name: text(fields.name, `${field}.name`),
    category: oneOf(findingCategories, fields.category, `${field}.category`),
    location: text(fields.location, `${field}.location`),
    path: text(fields.path, `${field}.path`),
    bytes: nonNegativeInteger(fields.bytes, `${field}.bytes`),
    files: nonNegativeInteger(fields.files, `${field}.files`),
    lowerBound: boolean(fields.lowerBound, `${field}.lowerBound`),
    modifiedMs: nullable(fields.modifiedMs, (time) => nonNegativeInteger(time, `${field}.modifiedMs`)),
    risk: oneOf(risks, fields.risk, `${field}.risk`),
    certainty: oneOf(certainties, fields.certainty, `${field}.certainty`),
    eligibility,
    whyNot,
    evidence: list(fields.evidence, `${field}.evidence`, MAX_EVIDENCE).map((entry, index) => evidence(entry, `${field}.evidence[${index}]`)),
    source: oneOf(["scan", "storage", "developer", "apps"] as const, fields.source, `${field}.source`),
    tool: nullable(fields.tool, (tool) => text(tool, `${field}.tool`)),
    fileCategory: nullable(fields.fileCategory, (category) => oneOf(fileCategories, category, `${field}.fileCategory`)),
  };
}

export function parseReview(payload: unknown): ReviewList {
  const fields = record(payload, "$");
  return {
    scan: nullable(fields.scan, (scan) => {
      const info = record(scan, "scan");
      return {
        mode: oneOf(scanModes, info.mode, "scan.mode"),
        startedAtMs: nonNegativeInteger(info.startedAtMs, "scan.startedAtMs"),
        cancelled: boolean(info.cancelled, "scan.cancelled"),
      };
    }),
    findings: list(fields.findings, "findings", MAX_FINDINGS).map((entry, index) => finding(entry, `findings[${index}]`)),
  };
}

export function parseCleanupScan(payload: unknown): CleanupScanResult {
  const fields = record(payload, "$");
  return {
    mode: oneOf(scanModes, fields.mode, "mode"),
    cancelled: boolean(fields.cancelled, "cancelled"),
    startedAtMs: nonNegativeInteger(fields.startedAtMs, "startedAtMs"),
    durationMs: nonNegativeInteger(fields.durationMs, "durationMs"),
    steps: list(fields.steps, "steps", stepIds.length).map((entry, index) => {
      const step = record(entry, `steps[${index}]`);
      return {
        id: oneOf(stepIds, step.id, `steps[${index}].id`),
        items: nonNegativeInteger(step.items, `steps[${index}].items`),
        bytes: nonNegativeInteger(step.bytes, `steps[${index}].bytes`),
        finished: boolean(step.finished, `steps[${index}].finished`),
      };
    }),
    needsFullDiskAccess: boolean(fields.needsFullDiskAccess, "needsFullDiskAccess"),
    skippedFolders: list(fields.skippedFolders, "skippedFolders", 100).map((folder, index) => text(folder, `skippedFolders[${index}]`)),
    docker: nullable(fields.docker, parseDockerState),
  };
}

export function parseCleanupProgress(payload: unknown): CleanupProgress {
  const fields = record(payload, "$");
  return {
    step: nonNegativeInteger(fields.step, "step"),
    items: nonNegativeInteger(fields.items, "items"),
    bytes: nonNegativeInteger(fields.bytes, "bytes"),
  };
}

export function parseScope(payload: unknown): ScopeFolder[] {
  return list(payload, "$", 100).map((entry, index) => {
    const folder = record(entry, `[${index}]`);
    return {
      id: oneOf(folderIds, folder.id, `[${index}].id`),
      path: text(folder.path, `[${index}].path`),
      access: oneOf(accesses, folder.access, `[${index}].access`),
      modes: list(folder.modes, `[${index}].modes`, 3).map((mode, at) => oneOf(scanModes, mode, `[${index}].modes[${at}]`)),
    };
  });
}

export function parseAccess(payload: unknown): FolderAccess {
  return oneOf(accesses, payload, "$");
}

export function parseAddedCleanupFolder(payload: unknown): AddedCleanupFolder {
  const fields = record(payload, "$");
  if (fields.status === "cancelled") {
    return { status: "cancelled" };
  }
  if (fields.status === "added") {
    return { status: "added", path: text(fields.path, "path") };
  }
  throw new ContractError("status");
}

export function parsePreview(payload: unknown): CleanupPreview {
  const fields = record(payload, "$");
  return {
    id: id(fields.id, "id"),
    preparedAtMs: nonNegativeInteger(fields.preparedAtMs, "preparedAtMs"),
    items: list(fields.items, "items", MAX_FINDINGS).map((entry, index) => {
      const item = record(entry, `items[${index}]`);
      return {
        id: id(item.id, `items[${index}].id`),
        state: oneOf(["planned", "changed", "notEligible"] as const, item.state, `items[${index}].state`),
        bytes: nonNegativeInteger(item.bytes, `items[${index}].bytes`),
      };
    }),
  };
}

export function parseMoveProgress(payload: unknown): MoveProgress {
  const fields = record(payload, "$");
  const itemId = id(fields.id, "id");
  const state = oneOf(["checking", "moved", "unconfirmed", "failed"] as const, fields.state, "state");
  if (state === "failed") {
    return { id: itemId, state, reason: oneOf(moveFailures, fields.reason, "reason") };
  }
  if (state === "moved" && fields.heldId !== undefined) {
    return { id: itemId, state, heldId: id(fields.heldId, "heldId") };
  }
  return { id: itemId, state };
}

export function parseApproveOutcome(payload: unknown): ApproveOutcome {
  const fields = record(payload, "$");
  const status = oneOf(["noPreview", "stale", "historyUnavailable", "finished"] as const, fields.status, "status");
  switch (status) {
    case "noPreview":
      return { status };
    case "stale":
      return { status, changed: ids(fields.changed, "changed") };
    case "historyUnavailable":
      return { status, reason: oneOf(historyReasons, fields.reason, "reason") };
    case "finished":
      return {
        status,
        batchId: id(fields.batchId, "batchId"),
        finishedAtMs: nonNegativeInteger(fields.finishedAtMs, "finishedAtMs"),
        items: list(fields.items, "items", MAX_FINDINGS).map((entry, index) => parseMoveProgress(record(entry, `items[${index}]`))),
      };
  }
}

export function parseQuarantine(payload: unknown): QuarantineList {
  const fields = record(payload, "$");
  const status = oneOf(["available", "unavailable"] as const, fields.status, "status");
  const folder = text(fields.folder, "folder");
  if (status === "unavailable") {
    return { status, reason: oneOf(historyReasons, fields.reason, "reason"), folder };
  }
  return {
    status,
    folder,
    batches: list(fields.batches, "batches", MAX_FINDINGS).map((entry, index) => {
      const batch = record(entry, `batches[${index}]`);
      return {
        id: id(batch.id, `batches[${index}].id`),
        createdMs: nonNegativeInteger(batch.createdMs, `batches[${index}].createdMs`),
        origin: oneOf(["standard", "deep", "dev", "sent"] as const, batch.origin, `batches[${index}].origin`),
        items: list(batch.items, `batches[${index}].items`, MAX_FINDINGS).map((value, at) => {
          const item = record(value, `batches[${index}].items[${at}]`);
          const field = `batches[${index}].items[${at}]`;
          return {
            id: id(item.id, `${field}.id`),
            name: text(item.name, `${field}.name`),
            category: oneOf(findingCategories, item.category, `${field}.category`),
            from: text(item.from, `${field}.from`),
            bytes: nonNegativeInteger(item.bytes, `${field}.bytes`),
            status: oneOf(itemStatuses, item.status, `${field}.status`),
            chooseFolder: boolean(item.chooseFolder, `${field}.chooseFolder`),
            ...(item.neededFolder === undefined ? {} : { neededFolder: text(item.neededFolder, `${field}.neededFolder`) }),
          };
        }),
      };
    }),
  };
}

export function parseRestoreResults(payload: unknown): RestoreResult[] {
  return list(payload, "$", MAX_FINDINGS).map((entry, index) => {
    const result = record(entry, `[${index}]`);
    return { id: id(result.id, `[${index}].id`), outcome: oneOf(restoreOutcomes, result.outcome, `[${index}].outcome`) };
  });
}

export function parseDeleteProgress(payload: unknown): DeleteProgress {
  const fields = record(payload, "$");
  return { id: id(fields.id, "id"), state: oneOf(["deleting", "deleted", "failed"] as const, fields.state, "state") };
}

export function parseDeleteSummary(payload: unknown): DeleteSummary {
  const fields = record(payload, "$");
  return {
    deleted: ids(fields.deleted, "deleted"),
    failed: ids(fields.failed, "failed"),
    kept: ids(fields.kept, "kept"),
    stopped: boolean(fields.stopped, "stopped"),
  };
}

export function parseIdList(payload: unknown): string[] {
  return ids(payload, "$");
}

export function parseBoolean(payload: unknown): boolean {
  return boolean(payload, "$");
}
