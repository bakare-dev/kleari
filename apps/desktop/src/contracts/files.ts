// IPC contract for the Files page. Mirrors src-tauri/src/files/; every response is validated
// before the UI sees it. Only opaque IDs, names and `~/…` locations arrive, never paths.

import { fileCategories, type FileCategory } from "./storage";
import { ContractError, nonNegativeInteger, nullable, oneOf, record } from "./systemMetrics";

export const tools = ["largeOld", "duplicates", "similar", "downloads", "ai"] as const;
export type Tool = (typeof tools)[number];

export type GroupKind = "large" | "old" | "copies" | "similar" | "source" | "ai";

export interface FileItem {
  id: string;
  name: string;
  location: string;
  bytes: number;
  lowerBound: boolean;
  folder: boolean;
  modifiedMs: number | null;
  openedMs: number | null;
  fileCategory: FileCategory | null;
  selectable: boolean;
}

export interface FileGroup {
  id: string;
  kind: GroupKind;
  label: string;
  detail: string | null;
  bytes: number;
  reclaimable: number;
  items: FileItem[];
  more: number;
}

export interface FilesScan {
  status: "completed" | "cancelled";
  tool: Tool;
  groups: FileGroup[];
  filesSeen: number;
  unreadable: number;
  truncated: boolean;
}

export interface FilesOptions {
  largeMinMb: 100 | 500 | 1000 | 5000;
  oldDays: 90 | 180 | 365 | 730;
}

const MAX_GROUPS = 1_000;
const MAX_ITEMS = 500;
const MAX_TEXT = 4096;

function text(value: unknown, field: string, allowEmpty = false): string {
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

export function parseFilesScan(payload: unknown): FilesScan {
  const fields = record(payload, "$");
  return {
    status: oneOf(["completed", "cancelled"] as const, fields.status, "status"),
    tool: oneOf(tools, fields.tool, "tool"),
    groups: list(fields.groups, "groups", MAX_GROUPS).map((entry, index) => {
      const field = `groups[${index}]`;
      const group = record(entry, field);
      return {
        id: id(group.id, `${field}.id`),
        kind: oneOf(["large", "old", "copies", "similar", "source", "ai"] as const, group.kind, `${field}.kind`),
        label: text(group.label, `${field}.label`, true),
        detail: nullable(group.detail, (detail) => text(detail, `${field}.detail`)),
        bytes: nonNegativeInteger(group.bytes, `${field}.bytes`),
        reclaimable: nonNegativeInteger(group.reclaimable, `${field}.reclaimable`),
        items: list(group.items, `${field}.items`, MAX_ITEMS).map((value, at) => {
          const itemField = `${field}.items[${at}]`;
          const item = record(value, itemField);
          return {
            id: id(item.id, `${itemField}.id`),
            name: text(item.name, `${itemField}.name`),
            location: text(item.location, `${itemField}.location`),
            bytes: nonNegativeInteger(item.bytes, `${itemField}.bytes`),
            lowerBound: boolean(item.lowerBound, `${itemField}.lowerBound`),
            folder: boolean(item.folder, `${itemField}.folder`),
            modifiedMs: nullable(item.modifiedMs, (ms) => nonNegativeInteger(ms, `${itemField}.modifiedMs`)),
            openedMs: nullable(item.openedMs, (ms) => nonNegativeInteger(ms, `${itemField}.openedMs`)),
            fileCategory: nullable(item.fileCategory, (category) => oneOf(fileCategories, category, `${itemField}.fileCategory`)),
            selectable: boolean(item.selectable, `${itemField}.selectable`),
          };
        }),
        more: nonNegativeInteger(group.more, `${field}.more`),
      };
    }),
    filesSeen: nonNegativeInteger(fields.filesSeen, "filesSeen"),
    unreadable: nonNegativeInteger(fields.unreadable, "unreadable"),
    truncated: boolean(fields.truncated, "truncated"),
  };
}

export function parseSent(payload: unknown): string[] {
  return list(payload, "$", 100_000).map((entry, index) => id(entry, `[${index}]`));
}
