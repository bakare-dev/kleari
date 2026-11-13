// IPC contract for the Apps page. Mirrors src-tauri/src/apps/ and apps_commands.rs; every
// response is validated before the UI sees it. Paths never arrive here: only opaque IDs,
// names and `~/…` locations.

import { ContractError, nonNegativeInteger, nullable, oneOf, record } from "./systemMetrics";

export interface LeftoverEntry {
  /** Set for a leftover of an app that is gone: what Send to review sends. */
  reviewId: string | null;
  name: string;
  location: string;
  /** The app it belongs to: its name, or its bundle ID when the app is gone. */
  owner: string;
  bytes: number;
  lowerBound: boolean;
  modifiedMs: number | null;
}

export interface AppEntry {
  id: string;
  name: string;
  bundleId: string | null;
  version: string | null;
  location: string;
  bytes: number;
  lowerBound: boolean;
  running: boolean;
  /** Part of macOS: never removed. */
  apple: boolean;
  leftovers: LeftoverEntry[];
  leftoverBytes: number;
}

export interface InstallerEntry {
  reviewId: string;
  name: string;
  location: string;
  bytes: number;
  modifiedMs: number | null;
  installedApp: string | null;
}

export interface AppsScan {
  status: "completed" | "cancelled";
  apps: AppEntry[];
  orphans: LeftoverEntry[];
  installers: InstallerEntry[];
}

export type RemoveOutcome =
  | { status: "removed"; leftovers: number; leftoverBytes: number }
  | { status: "running" }
  | { status: "refused" }
  | { status: "changed" }
  | { status: "failed" };

const MAX_APPS = 2_000;
const MAX_ENTRIES = 20_000;
const MAX_LEFTOVERS = 200;
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

function leftover(value: unknown, field: string): LeftoverEntry {
  const fields = record(value, field);
  return {
    reviewId: nullable(fields.reviewId, (reviewId) => id(reviewId, `${field}.reviewId`)),
    name: text(fields.name, `${field}.name`),
    location: text(fields.location, `${field}.location`),
    owner: text(fields.owner, `${field}.owner`),
    bytes: nonNegativeInteger(fields.bytes, `${field}.bytes`),
    lowerBound: boolean(fields.lowerBound, `${field}.lowerBound`),
    modifiedMs: nullable(fields.modifiedMs, (ms) => nonNegativeInteger(ms, `${field}.modifiedMs`)),
  };
}

export function parseAppsScan(payload: unknown): AppsScan {
  const fields = record(payload, "$");
  return {
    status: oneOf(["completed", "cancelled"] as const, fields.status, "status"),
    apps: list(fields.apps, "apps", MAX_APPS).map((entry, index) => {
      const field = `apps[${index}]`;
      const app = record(entry, field);
      return {
        id: id(app.id, `${field}.id`),
        name: text(app.name, `${field}.name`),
        bundleId: nullable(app.bundleId, (bundleId) => text(bundleId, `${field}.bundleId`)),
        version: nullable(app.version, (version) => text(version, `${field}.version`)),
        location: text(app.location, `${field}.location`),
        bytes: nonNegativeInteger(app.bytes, `${field}.bytes`),
        lowerBound: boolean(app.lowerBound, `${field}.lowerBound`),
        running: boolean(app.running, `${field}.running`),
        apple: boolean(app.apple, `${field}.apple`),
        leftovers: list(app.leftovers, `${field}.leftovers`, MAX_LEFTOVERS).map((left, at) => leftover(left, `${field}.leftovers[${at}]`)),
        leftoverBytes: nonNegativeInteger(app.leftoverBytes, `${field}.leftoverBytes`),
      };
    }),
    orphans: list(fields.orphans, "orphans", MAX_ENTRIES).map((entry, index) => leftover(entry, `orphans[${index}]`)),
    installers: list(fields.installers, "installers", MAX_ENTRIES).map((entry, index) => {
      const field = `installers[${index}]`;
      const installer = record(entry, field);
      return {
        reviewId: id(installer.reviewId, `${field}.reviewId`),
        name: text(installer.name, `${field}.name`),
        location: text(installer.location, `${field}.location`),
        bytes: nonNegativeInteger(installer.bytes, `${field}.bytes`),
        modifiedMs: nullable(installer.modifiedMs, (ms) => nonNegativeInteger(ms, `${field}.modifiedMs`)),
        installedApp: nullable(installer.installedApp, (name) => text(name, `${field}.installedApp`)),
      };
    }),
  };
}

export function parseRemoveOutcome(payload: unknown): RemoveOutcome {
  const fields = record(payload, "$");
  const status = oneOf(["removed", "running", "refused", "changed", "failed"] as const, fields.status, "status");
  if (status === "removed") {
    return {
      status,
      leftovers: nonNegativeInteger(fields.leftovers, "leftovers"),
      leftoverBytes: nonNegativeInteger(fields.leftoverBytes, "leftoverBytes"),
    };
  }
  return { status };
}

export function parseSentIds(payload: unknown): string[] {
  return list(payload, "$", MAX_ENTRIES).map((entry, index) => id(entry, `[${index}]`));
}
