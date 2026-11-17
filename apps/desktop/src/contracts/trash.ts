// IPC contract for the Trash tab. Mirrors src-tauri/src/trash/mod.rs; every response is
// validated before the UI sees it.

import { fileCategories, type FileCategory } from "./storage";
import { ContractError, nonNegativeInteger, nullable, oneOf, record } from "./systemMetrics";

export interface TrashItem {
  id: string;
  name: string;
  bytes: number;
  /** Something inside couldn't be read: a minimum. */
  lowerBound: boolean;
  folder: boolean;
  /** A file's kind, from its name; `null` for a folder. */
  fileCategory: FileCategory | null;
  /** When it was moved to the Trash (ms since 1970), if known. */
  trashedMs: number | null;
  /** Where it came from, as `~/…`, if Finder recorded it (Put Back is offered only then). */
  from: string | null;
  /** The external disk whose Trash it is in; `null` for your Mac's. */
  disk: string | null;
}

export type TrashList =
  | { status: "available"; items: TrashItem[]; bytes: number; lowerBound: boolean; more: number }
  | { status: "notAllowed" };

const failures = ["changed", "noOrigin", "exists", "folderGone", "needsAccess", "failed"] as const;
export type TrashFailure = (typeof failures)[number];

export interface TrashOutcome {
  done: string[];
  bytes: number;
  failed: { id: string; reason: TrashFailure }[];
}

const MAX_ITEMS = 50_000;
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

function list(value: unknown, field: string): unknown[] {
  if (!Array.isArray(value) || value.length > MAX_ITEMS) {
    throw new ContractError(field);
  }
  return value;
}

export function parseTrashList(payload: unknown): TrashList {
  const fields = record(payload, "$");
  const status = oneOf(["available", "notAllowed"] as const, fields.status, "status");
  if (status === "notAllowed") {
    return { status };
  }
  return {
    status,
    bytes: nonNegativeInteger(fields.bytes, "bytes"),
    lowerBound: boolean(fields.lowerBound, "lowerBound"),
    more: nonNegativeInteger(fields.more, "more"),
    items: list(fields.items, "items").map((entry, index) => {
      const item = record(entry, `items[${index}]`);
      const field = (name: string) => `items[${index}].${name}`;
      return {
        id: text(item.id, field("id")),
        name: text(item.name, field("name")),
        bytes: nonNegativeInteger(item.bytes, field("bytes")),
        lowerBound: boolean(item.lowerBound, field("lowerBound")),
        folder: boolean(item.folder, field("folder")),
        fileCategory: nullable(item.fileCategory, (value) => oneOf(fileCategories, value, field("fileCategory"))),
        trashedMs: nullable(item.trashedMs, (value) => nonNegativeInteger(value, field("trashedMs"))),
        from: nullable(item.from, (value) => text(value, field("from"))),
        disk: nullable(item.disk, (value) => text(value, field("disk"))),
      };
    }),
  };
}

export function parseTrashOutcome(payload: unknown): TrashOutcome {
  const fields = record(payload, "$");
  return {
    done: list(fields.done, "done").map((id, index) => text(id, `done[${index}]`)),
    bytes: nonNegativeInteger(fields.bytes, "bytes"),
    failed: list(fields.failed, "failed").map((entry, index) => {
      if (!Array.isArray(entry) || entry.length !== 2) {
        throw new ContractError(`failed[${index}]`);
      }
      return { id: text(entry[0], `failed[${index}][0]`), reason: oneOf(failures, entry[1], `failed[${index}][1]`) };
    }),
  };
}
