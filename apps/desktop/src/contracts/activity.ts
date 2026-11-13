// IPC contract for the Activity page. Mirrors src-tauri/src/activity.rs; every response is
// validated before the UI sees it. Items arrive with a name and their folder as `~/…`, never
// a full path.

import { findingCategories, historyReasons, type FindingCategory, type HistoryUnavailableReason } from "./cleanup";
import { ContractError, nonNegativeInteger, nullable, oneOf, record } from "./systemMetrics";

export const eventKinds = ["quarantine", "restore", "delete", "scan", "analysis", "developerScan", "review", "access", "quit"] as const;
export type EventKind = (typeof eventKinds)[number];

const MAX_EVENTS = 500;
const MAX_LINES = 20;
const MAX_TEXT = 1024;

export interface ActivityLine {
  name: string;
  category: FindingCategory;
  /** The folder it came from, as `~/…`. */
  folder: string;
  bytes: number;
  outcome: string;
}

export interface ActivityEvent {
  id: string;
  atMs: number;
  kind: EventKind;
  /** A scan mode, `developer`/`storage`, a protected folder's ID or an app's name. */
  subject: string | null;
  items: number;
  bytes: number;
  failed: number;
  outcome: string;
  lines: ActivityLine[];
  moreLines: number;
}

export type ActivityList =
  | { status: "available"; retentionDays: number; events: ActivityEvent[]; truncated: boolean }
  | { status: "unavailable"; reason: HistoryUnavailableReason };

function text(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_TEXT) {
    throw new ContractError(field);
  }
  return value;
}

/** Text that may be empty (an item recorded at the top of the disk has no folder to show). */
function maybeEmpty(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length > MAX_TEXT) {
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

function boolean(value: unknown, field: string): boolean {
  if (typeof value !== "boolean") {
    throw new ContractError(field);
  }
  return value;
}

/** Outcome codes are short camelCase words from Rust, never free text. */
function code(value: unknown, field: string): string {
  if (typeof value !== "string" || !/^[a-z][A-Za-z]{0,31}$/.test(value)) {
    throw new ContractError(field);
  }
  return value;
}

export function parseActivity(payload: unknown): ActivityList {
  const fields = record(payload, "$");
  const status = oneOf(["available", "unavailable"] as const, fields.status, "status");
  if (status === "unavailable") {
    return { status, reason: oneOf(historyReasons, fields.reason, "reason") };
  }
  return {
    status,
    retentionDays: nonNegativeInteger(fields.retentionDays, "retentionDays"),
    truncated: boolean(fields.truncated, "truncated"),
    events: list(fields.events, "events", MAX_EVENTS).map((value, index) => {
      const event = record(value, `events[${index}]`);
      const field = `events[${index}]`;
      const eventId = text(event.id, `${field}.id`);
      if (!/^[0-9]{1,19}$/.test(eventId)) {
        throw new ContractError(`${field}.id`);
      }
      return {
        id: eventId,
        atMs: nonNegativeInteger(event.atMs, `${field}.atMs`),
        kind: oneOf(eventKinds, event.kind, `${field}.kind`),
        subject: nullable(event.subject, (subject) => text(subject, `${field}.subject`)),
        items: nonNegativeInteger(event.items, `${field}.items`),
        bytes: nonNegativeInteger(event.bytes, `${field}.bytes`),
        failed: nonNegativeInteger(event.failed, `${field}.failed`),
        outcome: code(event.outcome, `${field}.outcome`),
        moreLines: nonNegativeInteger(event.moreLines, `${field}.moreLines`),
        lines: list(event.lines, `${field}.lines`, MAX_LINES).map((entry, at) => {
          const line = record(entry, `${field}.lines[${at}]`);
          const lineField = `${field}.lines[${at}]`;
          return {
            name: text(line.name, `${lineField}.name`),
            category: oneOf(findingCategories, line.category, `${lineField}.category`),
            folder: maybeEmpty(line.folder, `${lineField}.folder`),
            bytes: nonNegativeInteger(line.bytes, `${lineField}.bytes`),
            outcome: code(line.outcome, `${lineField}.outcome`),
          };
        }),
      };
    }),
  };
}
