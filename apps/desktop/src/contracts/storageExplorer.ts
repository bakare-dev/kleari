// IPC contract for the Storage page's Browse view. Mirrors src-tauri/src/storage/explorer.rs;
// every response is validated before the UI sees it. Only opaque IDs, names and `~/…`
// locations arrive here, never paths.

import { fileCategories, type FileCategory } from "./storage";
import { ContractError, nonNegativeInteger, nullable, oneOf, record } from "./systemMetrics";

export type EntryKind = "folder" | "file" | "link" | "excluded";

export interface ExplorerEntry {
  id: string;
  name: string;
  kind: EntryKind;
  bytes: number;
  lowerBound: boolean;
  files: number;
  modifiedMs: number | null;
  fileCategory: FileCategory | null;
  selectable: boolean;
}

export interface Crumb {
  id: string;
  name: string;
}

export type ListingStatus = "completed" | "cancelled" | "changed" | "unreadable";

export interface Listing {
  status: ListingStatus;
  id: string;
  name: string;
  location: string;
  crumbs: Crumb[];
  bytes: number;
  lowerBound: boolean;
  entries: ExplorerEntry[];
  moreEntries: number;
  moreBytes: number;
}

export interface ListingProgress {
  done: number;
  total: number;
}

const MAX_ENTRIES = 2_000;
const MAX_CRUMBS = 256;
const MAX_TEXT = 4096;

function text(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_TEXT) {
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

function id(value: unknown, field: string): string {
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

export function parseListing(payload: unknown): Listing | null {
  return nullable(payload, (value) => {
    const fields = record(value, "$");
    return {
      status: oneOf(["completed", "cancelled", "changed", "unreadable"] as const, fields.status, "status"),
      id: id(fields.id, "id"),
      name: text(fields.name, "name"),
      location: text(fields.location, "location"),
      crumbs: list(fields.crumbs, "crumbs", MAX_CRUMBS).map((entry, index) => {
        const crumb = record(entry, `crumbs[${index}]`);
        return { id: id(crumb.id, `crumbs[${index}].id`), name: text(crumb.name, `crumbs[${index}].name`) };
      }),
      bytes: nonNegativeInteger(fields.bytes, "bytes"),
      lowerBound: boolean(fields.lowerBound, "lowerBound"),
      entries: list(fields.entries, "entries", MAX_ENTRIES).map((entry, index) => {
        const field = `entries[${index}]`;
        const item = record(entry, field);
        return {
          id: id(item.id, `${field}.id`),
          name: text(item.name, `${field}.name`),
          kind: oneOf(["folder", "file", "link", "excluded"] as const, item.kind, `${field}.kind`),
          bytes: nonNegativeInteger(item.bytes, `${field}.bytes`),
          lowerBound: boolean(item.lowerBound, `${field}.lowerBound`),
          files: nonNegativeInteger(item.files, `${field}.files`),
          modifiedMs: nullable(item.modifiedMs, (ms) => nonNegativeInteger(ms, `${field}.modifiedMs`)),
          fileCategory: nullable(item.fileCategory, (category) => oneOf(fileCategories, category, `${field}.fileCategory`)),
          selectable: boolean(item.selectable, `${field}.selectable`),
        };
      }),
      moreEntries: nonNegativeInteger(fields.moreEntries, "moreEntries"),
      moreBytes: nonNegativeInteger(fields.moreBytes, "moreBytes"),
    };
  });
}

export function parseListingProgress(payload: unknown): ListingProgress {
  const fields = record(payload, "$");
  return { done: nonNegativeInteger(fields.done, "done"), total: nonNegativeInteger(fields.total, "total") };
}

export function parseIds(payload: unknown): string[] {
  return list(payload, "$", MAX_ENTRIES).map((entry, index) => id(entry, `[${index}]`));
}
