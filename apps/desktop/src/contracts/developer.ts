// IPC contracts for the Developer page. Mirrors src-tauri/src/developer/; every response is
// validated before the UI sees it.

import { ContractError, nonNegativeInteger, nullable, oneOf, record } from "./systemMetrics";

export type ProjectStack = "node" | "nextJs" | "rust" | "maven" | "gradle" | "python" | "cocoaPods" | "flutter" | "php";

export const projectStacks: readonly ProjectStack[] = ["node", "nextJs", "rust", "maven", "gradle", "python", "cocoaPods", "flutter", "php"];

export interface MeasuredPart {
  /** Empty for the combined rest. */
  name: string;
  bytes: number;
  items: number;
}

export interface Measured {
  bytes: number;
  files: number;
  lowerBound: boolean;
  parts: MeasuredPart[];
}

export interface CacheResult {
  id: string;
  locations: string[];
  version: string | null;
  measured: Measured;
  /** Sends it to Cleanup's review; null when the backend couldn't identify it. */
  reviewId: string | null;
}

export interface BuildFolder {
  name: string;
  measured: Measured;
  reviewId: string | null;
}

export interface ProjectResult {
  name: string;
  location: string;
  stack: ProjectStack;
  lastCommitMs: number | null;
  /** A file in it, or the folder, was open in some app when scanned; nothing is suggested. */
  inUse: boolean;
  bytes: number;
  folders: BuildFolder[];
}

export interface DockerUsage {
  imagesBytes: number;
  buildCacheBytes: number;
  volumesBytes: number;
  containersBytes: number;
  /** Every figure was reported; otherwise the totals are a minimum. */
  complete: boolean;
}

export type DockerState =
  | { status: "notInstalled" }
  | { status: "notChecked" }
  | { status: "notRunning"; diskImageBytes: number | null }
  | { status: "running"; usage: DockerUsage; diskImageBytes: number | null };

export interface DeveloperScan {
  status: "completed" | "cancelled";
  startedAtMs: number;
  durationMs: number;
  caches: CacheResult[];
  projects: ProjectResult[];
  projectsTruncated: boolean;
  unreadableFolders: number;
  roots: string[];
  /** Roots that couldn't be searched. */
  skippedRoots: string[];
  docker: DockerState;
}

export interface DeveloperProgress {
  phase: "caches" | "projects" | "docker";
  done: number;
}

export type AddedFolder = { status: "added"; name: string } | { status: "cancelled" };

/** Bounds well above the backend's own (500 projects, 19 caches, 4 parts per folder). */
const MAX_CACHES = 50;
const MAX_PROJECTS = 1_000;
const MAX_PARTS = 10;
const MAX_FOLDERS = 20;
const MAX_TEXT = 4096;

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

function reviewId(value: unknown, field: string): string | null {
  return nullable(value, (id) => {
    if (typeof id !== "string" || !/^[0-9a-f]{1,64}$/.test(id)) {
      throw new ContractError(field);
    }
    return id;
  });
}

function list(value: unknown, field: string, max: number): unknown[] {
  if (!Array.isArray(value) || value.length > max) {
    throw new ContractError(field);
  }
  return value;
}

function measured(value: unknown, field: string): Measured {
  const fields = record(value, field);
  const bytes = nonNegativeInteger(fields.bytes, `${field}.bytes`);
  const parts = list(fields.parts, `${field}.parts`, MAX_PARTS).map((entry, index) => {
    const part = record(entry, `${field}.parts[${index}]`);
    return {
      name: text(part.name, `${field}.parts[${index}].name`, { allowEmpty: true }),
      bytes: nonNegativeInteger(part.bytes, `${field}.parts[${index}].bytes`),
      items: nonNegativeInteger(part.items, `${field}.parts[${index}].items`),
    };
  });
  // Parts are what the folder holds, so together they can't be larger than it.
  if (parts.reduce((total, part) => total + part.bytes, 0) > bytes) {
    throw new ContractError(`${field}.parts`);
  }
  return { bytes, files: nonNegativeInteger(fields.files, `${field}.files`), lowerBound: boolean(fields.lowerBound, `${field}.lowerBound`), parts };
}

/** Docker's disk use as the backend reports it; shared with the Cleanup scan. */
export function parseDockerState(value: unknown): DockerState {
  return docker(value);
}

function docker(value: unknown): DockerState {
  const fields = record(value, "docker");
  const status = oneOf(["notInstalled", "notChecked", "notRunning", "running"] as const, fields.status, "docker.status");
  if (status === "notInstalled" || status === "notChecked") {
    return { status };
  }
  const diskImageBytes = nullable(fields.diskImageBytes, (bytes) => nonNegativeInteger(bytes, "docker.diskImageBytes"));
  if (status === "notRunning") {
    return { status, diskImageBytes };
  }
  const usage = record(fields.usage, "docker.usage");
  return {
    status,
    diskImageBytes,
    usage: {
      imagesBytes: nonNegativeInteger(usage.imagesBytes, "docker.usage.imagesBytes"),
      buildCacheBytes: nonNegativeInteger(usage.buildCacheBytes, "docker.usage.buildCacheBytes"),
      volumesBytes: nonNegativeInteger(usage.volumesBytes, "docker.usage.volumesBytes"),
      containersBytes: nonNegativeInteger(usage.containersBytes, "docker.usage.containersBytes"),
      complete: boolean(usage.complete, "docker.usage.complete"),
    },
  };
}

export function parseDeveloperScan(payload: unknown): DeveloperScan {
  const fields = record(payload, "$");
  return {
    status: oneOf(["completed", "cancelled"] as const, fields.status, "status"),
    startedAtMs: nonNegativeInteger(fields.startedAtMs, "startedAtMs"),
    durationMs: nonNegativeInteger(fields.durationMs, "durationMs"),
    caches: list(fields.caches, "caches", MAX_CACHES).map((entry, index) => {
      const cache = record(entry, `caches[${index}]`);
      return {
        id: text(cache.id, `caches[${index}].id`),
        locations: list(cache.locations, `caches[${index}].locations`, 10).map((location, at) => text(location, `caches[${index}].locations[${at}]`)),
        version: nullable(cache.version, (version) => text(version, `caches[${index}].version`)),
        measured: measured(cache.measured, `caches[${index}].measured`),
        reviewId: reviewId(cache.reviewId, `caches[${index}].reviewId`),
      };
    }),
    projects: list(fields.projects, "projects", MAX_PROJECTS).map((entry, index) => {
      const project = record(entry, `projects[${index}]`);
      const folders = list(project.folders, `projects[${index}].folders`, MAX_FOLDERS).map((folder, at) => {
        const fieldsOfFolder = record(folder, `projects[${index}].folders[${at}]`);
        return {
          name: text(fieldsOfFolder.name, `projects[${index}].folders[${at}].name`),
          measured: measured(fieldsOfFolder.measured, `projects[${index}].folders[${at}].measured`),
          reviewId: reviewId(fieldsOfFolder.reviewId, `projects[${index}].folders[${at}].reviewId`),
        };
      });
      const bytes = nonNegativeInteger(project.bytes, `projects[${index}].bytes`);
      if (bytes !== folders.reduce((total, folder) => total + folder.measured.bytes, 0)) {
        throw new ContractError(`projects[${index}].bytes`);
      }
      return {
        name: text(project.name, `projects[${index}].name`),
        location: text(project.location, `projects[${index}].location`),
        stack: oneOf(projectStacks, project.stack, `projects[${index}].stack`),
        lastCommitMs: nullable(project.lastCommitMs, (time) => nonNegativeInteger(time, `projects[${index}].lastCommitMs`)),
        inUse: boolean(project.inUse, `projects[${index}].inUse`),
        bytes,
        folders,
      };
    }),
    projectsTruncated: boolean(fields.projectsTruncated, "projectsTruncated"),
    unreadableFolders: nonNegativeInteger(fields.unreadableFolders, "unreadableFolders"),
    roots: list(fields.roots, "roots", 100).map((root, index) => text(root, `roots[${index}]`)),
    skippedRoots: list(fields.skippedRoots, "skippedRoots", 100).map((root, index) => text(root, `skippedRoots[${index}]`)),
    docker: docker(fields.docker),
  };
}

export function parseDeveloperProgress(payload: unknown): DeveloperProgress {
  const fields = record(payload, "$");
  return {
    phase: oneOf(["caches", "projects", "docker"] as const, fields.phase, "phase"),
    done: nonNegativeInteger(fields.done, "done"),
  };
}

export function parseAddedFolder(payload: unknown): AddedFolder {
  const fields = record(payload, "$");
  if (fields.status === "cancelled") {
    return { status: "cancelled" };
  }
  if (fields.status === "added") {
    return { status: "added", name: text(fields.name, "name") };
  }
  throw new ContractError("status");
}
