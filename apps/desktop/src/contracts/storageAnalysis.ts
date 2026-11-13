// IPC contracts for the Storage page: startup-volume capacity and the fixed-location analysis.
// Mirrors src-tauri/src/storage/{volume.rs,analysis/}; every response is validated before the
// UI sees it.

import {
  ContractError,
  nonNegativeInteger,
  nullable,
  oneOf,
  readingOf,
  reading,
  record,
  type Reading,
} from "./systemMetrics";

export type StorageCategoryId = "applications" | "developer" | "user" | "system" | "caches" | "other";
export type AnalysisStepId = "applications" | "home" | "system" | "libraryOptVar";
export type FileLock = "docker" | "xcodeManaged" | "macos";
export type AccessStatus = "granted" | "notGranted" | "unknown";

export const storageCategoryIds: readonly StorageCategoryId[] = ["applications", "developer", "user", "system", "caches", "other"];
export const analysisStepIds: readonly AnalysisStepId[] = ["applications", "home", "system", "libraryOptVar"];

export interface VolumeSpace {
  name: string;
  totalBytes: number;
  /** As Finder counts it: free space plus space macOS can purge. */
  availableBytes: number;
  usedBytes: number;
}

export interface StorageOverview {
  volume: Reading<VolumeSpace>;
  /** Estimate: Finder's available space minus space free right now. */
  purgeableBytes: Reading<number>;
  swapAndSleepBytes: Reading<number>;
  fullDiskAccess: AccessStatus;
}

export interface AnalysisNode {
  /** Empty for `others`. */
  name: string;
  kind: "folder" | "others";
  bytes: number;
  category: StorageCategoryId;
  /** Something inside couldn't be read, so the size is a minimum. */
  lowerBound: boolean;
  children: AnalysisNode[] | null;
}

export interface AnalysisStepResult {
  id: AnalysisStepId;
  bytes: number;
  lowerBound: boolean;
  node: AnalysisNode | null;
}

export interface CategoryTotal {
  id: StorageCategoryId;
  bytes: number;
  /** Something that would have counted toward it wasn't counted, so `bytes` is a minimum. */
  lowerBound: boolean;
  top: { label: string; bytes: number }[];
}

export interface AnalysisLargeFile {
  name: string;
  folder: string;
  bytes: number;
  modifiedMs: number | null;
  category: StorageCategoryId;
  lock: FileLock | null;
  /** Adds it to Cleanup's review (inspect only). */
  reviewId: string;
}

export interface AnalysisCoverage {
  unreadable: number;
  symlinksSkipped: number;
  otherVolumesSkipped: number;
  tooDeep: number;
  /** Hard links past the tracking limit, not counted because they may duplicate a file. */
  hardLinksSkipped: number;
}

export interface StorageAnalysisResult {
  status: "completed" | "cancelled";
  startedAtMs: number;
  durationMs: number;
  steps: AnalysisStepResult[];
  categories: CategoryTotal[];
  largeFiles: AnalysisLargeFile[];
  largeFilesTruncated: boolean;
  coverage: AnalysisCoverage;
}

export interface AnalysisProgress {
  step: AnalysisStepId;
  countedBytes: number;
  /** Bytes counted by each finished step, in order. */
  finishedSteps: number[];
}

/** Bounds that keep a malformed reply from flooding the UI. They sit well above the backend's own limits. */
const MAX_TEXT = 4096;
const MAX_TREE_NODES = 50_000;
const MAX_TREE_DEPTH = 12;
const MAX_LARGE_FILES = 50;
const MAX_CATEGORY_ITEMS = 10;

function text(value: unknown, field: string, { allowEmpty = false } = {}): string {
  if (typeof value !== "string" || value.length > MAX_TEXT || (!allowEmpty && value.length === 0)) {
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

/** An opaque ID from the backend: hex digits only. */
function reviewId(value: unknown, field: string): string {
  if (typeof value !== "string" || !/^[0-9a-f]{1,64}$/.test(value)) {
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

const accessStatuses: readonly AccessStatus[] = ["granted", "notGranted", "unknown"];
const locks: readonly FileLock[] = ["docker", "xcodeManaged", "macos"];

export function parseStorageOverview(payload: unknown): StorageOverview {
  const fields = record(payload, "$");
  return {
    volume: reading(fields.volume, "volume", (volume) => {
      const totalBytes = nonNegativeInteger(volume.totalBytes, "volume.totalBytes");
      const availableBytes = nonNegativeInteger(volume.availableBytes, "volume.availableBytes");
      const usedBytes = nonNegativeInteger(volume.usedBytes, "volume.usedBytes");
      if (totalBytes === 0 || availableBytes > totalBytes || usedBytes !== totalBytes - availableBytes) {
        throw new ContractError("volume");
      }
      return { name: text(volume.name, "volume.name"), totalBytes, availableBytes, usedBytes };
    }),
    purgeableBytes: readingOf(fields.purgeableBytes, "purgeableBytes", (value) => nonNegativeInteger(value, "purgeableBytes.value")),
    swapAndSleepBytes: readingOf(fields.swapAndSleepBytes, "swapAndSleepBytes", (value) => nonNegativeInteger(value, "swapAndSleepBytes.value")),
    fullDiskAccess: oneOf(accessStatuses, fields.fullDiskAccess, "fullDiskAccess"),
  };
}

function parseNode(value: unknown, field: string, depth: number, budget: { nodes: number }): AnalysisNode {
  budget.nodes += 1;
  if (budget.nodes > MAX_TREE_NODES || depth > MAX_TREE_DEPTH) {
    throw new ContractError(field);
  }
  const fields = record(value, field);
  const kind = oneOf(["folder", "others"] as const, fields.kind, `${field}.kind`);
  const bytes = nonNegativeInteger(fields.bytes, `${field}.bytes`);
  const children =
    fields.children === null
      ? null
      : list(fields.children, `${field}.children`, MAX_TREE_NODES).map((child, index) => parseNode(child, `${field}.children[${index}]`, depth + 1, budget));
  // Children are this folder's contents, so together they can't be larger than it.
  if (children !== null && children.reduce((total, child) => total + child.bytes, 0) > bytes) {
    throw new ContractError(`${field}.children`);
  }
  return {
    name: text(fields.name, `${field}.name`, { allowEmpty: kind === "others" }),
    kind,
    bytes,
    category: oneOf(storageCategoryIds, fields.category, `${field}.category`),
    lowerBound: boolean(fields.lowerBound, `${field}.lowerBound`),
    children,
  };
}

export function parseStorageAnalysis(payload: unknown): StorageAnalysisResult {
  const fields = record(payload, "$");
  const budget = { nodes: 0 };
  const steps = list(fields.steps, "steps", analysisStepIds.length).map((entry, index) => {
    const step = record(entry, `steps[${index}]`);
    const node = nullable(step.node, (value) => parseNode(value, `steps[${index}].node`, 0, budget));
    const bytes = nonNegativeInteger(step.bytes, `steps[${index}].bytes`);
    if (bytes !== (node?.bytes ?? 0)) {
      throw new ContractError(`steps[${index}].bytes`);
    }
    return {
      id: oneOf(analysisStepIds, step.id, `steps[${index}].id`),
      bytes,
      lowerBound: boolean(step.lowerBound, `steps[${index}].lowerBound`),
      node,
    };
  });
  const categories = list(fields.categories, "categories", storageCategoryIds.length).map((entry, index) => {
    const category = record(entry, `categories[${index}]`);
    return {
      id: oneOf(storageCategoryIds, category.id, `categories[${index}].id`),
      bytes: nonNegativeInteger(category.bytes, `categories[${index}].bytes`),
      lowerBound: boolean(category.lowerBound, `categories[${index}].lowerBound`),
      top: list(category.top, `categories[${index}].top`, MAX_CATEGORY_ITEMS).map((item, position) => {
        const fieldsOfItem = record(item, `categories[${index}].top[${position}]`);
        return {
          label: text(fieldsOfItem.label, `categories[${index}].top[${position}].label`),
          bytes: nonNegativeInteger(fieldsOfItem.bytes, `categories[${index}].top[${position}].bytes`),
        };
      }),
    };
  });
  const coverage = record(fields.coverage, "coverage");
  return {
    status: oneOf(["completed", "cancelled"] as const, fields.status, "status"),
    startedAtMs: nonNegativeInteger(fields.startedAtMs, "startedAtMs"),
    durationMs: nonNegativeInteger(fields.durationMs, "durationMs"),
    steps,
    categories,
    largeFiles: list(fields.largeFiles, "largeFiles", MAX_LARGE_FILES).map((entry, index) => {
      const file = record(entry, `largeFiles[${index}]`);
      return {
        name: text(file.name, `largeFiles[${index}].name`),
        folder: text(file.folder, `largeFiles[${index}].folder`),
        bytes: nonNegativeInteger(file.bytes, `largeFiles[${index}].bytes`),
        modifiedMs: nullable(file.modifiedMs, (value) => nonNegativeInteger(value, `largeFiles[${index}].modifiedMs`)),
        category: oneOf(storageCategoryIds, file.category, `largeFiles[${index}].category`),
        lock: nullable(file.lock, (value) => oneOf(locks, value, `largeFiles[${index}].lock`)),
        reviewId: reviewId(file.reviewId, `largeFiles[${index}].reviewId`),
      };
    }),
    largeFilesTruncated: boolean(fields.largeFilesTruncated, "largeFilesTruncated"),
    coverage: {
      unreadable: nonNegativeInteger(coverage.unreadable, "coverage.unreadable"),
      symlinksSkipped: nonNegativeInteger(coverage.symlinksSkipped, "coverage.symlinksSkipped"),
      otherVolumesSkipped: nonNegativeInteger(coverage.otherVolumesSkipped, "coverage.otherVolumesSkipped"),
      tooDeep: nonNegativeInteger(coverage.tooDeep, "coverage.tooDeep"),
      hardLinksSkipped: nonNegativeInteger(coverage.hardLinksSkipped, "coverage.hardLinksSkipped"),
    },
  };
}

export function parseAnalysisProgress(payload: unknown): AnalysisProgress {
  const fields = record(payload, "$");
  return {
    step: oneOf(analysisStepIds, fields.step, "step"),
    countedBytes: nonNegativeInteger(fields.countedBytes, "countedBytes"),
    finishedSteps: list(fields.finishedSteps, "finishedSteps", analysisStepIds.length).map((value, index) => nonNegativeInteger(value, `finishedSteps[${index}]`)),
  };
}
