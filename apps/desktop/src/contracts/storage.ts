// Mirror of the Rust `storage` DTOs. Payloads are untrusted, so each is validated at runtime.

export type FileCategory =
  | "video"
  | "image"
  | "audio"
  | "archive"
  | "diskImage"
  | "document"
  | "code"
  | "other";

export const fileCategories: readonly FileCategory[] = [
  "video",
  "image",
  "audio",
  "archive",
  "diskImage",
  "document",
  "code",
  "other",
];

/** Mirrors `storage::RESULT_LIMIT` in Rust. */
export const RESULT_LIMIT = 50;

/** Mirrors the backend's 100 top-level rows plus the combined "others" row. */
export const MAX_TOP_LEVEL_ROWS = 101;

export type FolderChoice =
  | { status: "chosen"; grantId: string; displayName: string }
  | { status: "cancelled" };

export interface ScanProgress {
  files: number;
  bytesOnDisk: number;
  /** Current folder relative to the chosen folder; empty for the folder itself. */
  current: string;
}

export interface FileEntry {
  /** Relative to the chosen folder; invalid UTF-8 bytes are shown as `\x{XX}`. */
  relativePath: string;
  bytesOnDisk: number;
  category: FileCategory;
  /** `id` of the top-level row the file belongs to. */
  topLevel: number;
}

export interface QueryResult {
  files: FileEntry[];
  matchedCount: number;
  indexTruncated: boolean;
}

export interface TopLevelEntry {
  /** Stable within one scan; used for filtering, so names need not be unique. */
  id: number;
  /** Empty for the "files" and "others" rows. */
  name: string;
  kind: "folder" | "files" | "others";
  bytesOnDisk: number;
  files: number;
  /** Items inside that couldn't be read; when non-zero the size is a lower bound. */
  unreadable: number;
  /** Top-level items this row covers (more than one for "others"). */
  items: number;
}

export interface ScanSummary {
  grantId: string;
  displayName: string;
  status: "completed" | "cancelled";
  startedAtMs: number;
  durationMs: number;
  totals: { bytesOnDisk: number; files: number; directories: number };
  coverage: {
    unreadable: number;
    symlinksSkipped: number;
    otherVolumesSkipped: number;
    tooDeep: number;
    hardLinksUnverified: number;
  };
  topLevel: TopLevelEntry[];
  indexedFiles: number;
  largest: QueryResult;
}

export interface FileFilter {
  name?: string;
  category?: FileCategory;
  /** A top-level row `id`. */
  topLevel?: number;
  minBytes?: number;
}

export class StorageContractError extends Error {
  constructor(field: string) {
    super(`Invalid storage payload at "${field}"`);
    this.name = "StorageContractError";
  }
}

type Fields = Record<string, unknown>;

function isFields(value: unknown): value is Fields {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function record(value: unknown, field: string): Fields {
  if (!isFields(value)) {
    throw new StorageContractError(field);
  }
  return value;
}

function count(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new StorageContractError(field);
  }
  return value;
}

function text(value: unknown, field: string, { allowEmpty }: { allowEmpty: boolean }): string {
  if (typeof value !== "string" || (!allowEmpty && value.length === 0)) {
    throw new StorageContractError(field);
  }
  return value;
}

function bool(value: unknown, field: string): boolean {
  if (typeof value !== "boolean") {
    throw new StorageContractError(field);
  }
  return value;
}

function oneOf<T extends string>(allowed: readonly T[], value: unknown, field: string): T {
  const match = allowed.find((candidate) => candidate === value);
  if (match === undefined) {
    throw new StorageContractError(field);
  }
  return match;
}

export function parseFolderChoice(payload: unknown): FolderChoice {
  const fields = record(payload, "$");
  if (fields.status === "cancelled") {
    return { status: "cancelled" };
  }
  if (fields.status === "chosen") {
    return {
      status: "chosen",
      grantId: text(fields.grantId, "grantId", { allowEmpty: false }),
      displayName: text(fields.displayName, "displayName", { allowEmpty: false }),
    };
  }
  throw new StorageContractError("status");
}

export function parseProgress(payload: unknown): ScanProgress {
  const fields = record(payload, "$");
  return {
    files: count(fields.files, "files"),
    bytesOnDisk: count(fields.bytesOnDisk, "bytesOnDisk"),
    current: text(fields.current, "current", { allowEmpty: true }),
  };
}

function fileEntry(value: unknown, field: string): FileEntry {
  const fields = record(value, field);
  return {
    relativePath: text(fields.relativePath, `${field}.relativePath`, { allowEmpty: false }),
    bytesOnDisk: count(fields.bytesOnDisk, `${field}.bytesOnDisk`),
    category: oneOf(fileCategories, fields.category, `${field}.category`),
    topLevel: count(fields.topLevel, `${field}.topLevel`),
  };
}

export function parseQueryResult(payload: unknown, field = "$"): QueryResult {
  const fields = record(payload, field);
  if (!Array.isArray(fields.files) || fields.files.length > RESULT_LIMIT) {
    throw new StorageContractError(`${field}.files`);
  }
  const files = fields.files.map((entry, index) => fileEntry(entry, `${field}.files[${index}]`));
  const matchedCount = count(fields.matchedCount, `${field}.matchedCount`);
  if (matchedCount < files.length) {
    throw new StorageContractError(`${field}.matchedCount`);
  }
  return { files, matchedCount, indexTruncated: bool(fields.indexTruncated, `${field}.indexTruncated`) };
}

function topLevelEntry(value: unknown, field: string): TopLevelEntry {
  const fields = record(value, field);
  const kind = oneOf(["folder", "files", "others"] as const, fields.kind, `${field}.kind`);
  return {
    id: count(fields.id, `${field}.id`),
    name: text(fields.name, `${field}.name`, { allowEmpty: kind !== "folder" }),
    kind,
    bytesOnDisk: count(fields.bytesOnDisk, `${field}.bytesOnDisk`),
    files: count(fields.files, `${field}.files`),
    unreadable: count(fields.unreadable, `${field}.unreadable`),
    items: count(fields.items, `${field}.items`),
  };
}

export function parseScanSummary(payload: unknown): ScanSummary {
  const fields = record(payload, "$");
  const totals = record(fields.totals, "totals");
  const coverage = record(fields.coverage, "coverage");
  if (!Array.isArray(fields.topLevel) || fields.topLevel.length > MAX_TOP_LEVEL_ROWS) {
    throw new StorageContractError("topLevel");
  }
  return {
    grantId: text(fields.grantId, "grantId", { allowEmpty: false }),
    displayName: text(fields.displayName, "displayName", { allowEmpty: false }),
    status: oneOf(["completed", "cancelled"] as const, fields.status, "status"),
    startedAtMs: count(fields.startedAtMs, "startedAtMs"),
    durationMs: count(fields.durationMs, "durationMs"),
    totals: {
      bytesOnDisk: count(totals.bytesOnDisk, "totals.bytesOnDisk"),
      files: count(totals.files, "totals.files"),
      directories: count(totals.directories, "totals.directories"),
    },
    coverage: {
      unreadable: count(coverage.unreadable, "coverage.unreadable"),
      symlinksSkipped: count(coverage.symlinksSkipped, "coverage.symlinksSkipped"),
      otherVolumesSkipped: count(coverage.otherVolumesSkipped, "coverage.otherVolumesSkipped"),
      tooDeep: count(coverage.tooDeep, "coverage.tooDeep"),
      hardLinksUnverified: count(coverage.hardLinksUnverified, "coverage.hardLinksUnverified"),
    },
    topLevel: fields.topLevel.map((entry, index) => topLevelEntry(entry, `topLevel[${index}]`)),
    indexedFiles: count(fields.indexedFiles, "indexedFiles"),
    largest: parseQueryResult(fields.largest, "largest"),
  };
}
