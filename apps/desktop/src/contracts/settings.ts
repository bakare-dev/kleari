// IPC contract for the Settings page. Mirrors src-tauri/src/settings_commands.rs and
// src-tauri/src/preferences.rs; every response is validated before the UI sees it. Folders
// arrive as `~/…` and are only ever sent back to name one to remove.

import { scanModes, type ScanMode, type ScanOptionsInput } from "./cleanup";
import { ContractError, nonNegativeInteger, oneOf, record } from "./systemMetrics";

export interface Preferences {
  defaultScan: ScanMode;
  scanDefaults: ScanOptionsInput;
  /** 0 is off. */
  minSizeMb: number;
  /** 0 is off. */
  skipRecentDays: number;
  /** 0 is off. */
  quarantineReminderGb: number;
  notifyScan: boolean;
  notifyQuarantine: boolean;
  notifySound: boolean;
  retentionDays: number;
  /** Kleari sits in the menu bar and keeps running when its window is closed. */
  menuBar: boolean;
  /** Check for a newer Kleari at launch. */
  checkForUpdates: boolean;
  /** Also offer beta releases, not only stable ones; `null` until chosen, when it follows
   * the installed version (a beta gets betas). */
  betaUpdates: boolean | null;
  /** When Kleari scans on its own (read only), then notifies. */
  scanSchedule: ScanSchedule;
}

export const scheduleFrequencies = ["off", "daily", "weekly"] as const;
export type ScheduleFrequency = (typeof scheduleFrequencies)[number];

export interface ScanSchedule {
  frequency: ScheduleFrequency;
  /** For a weekly scan: 0 is Monday, 6 is Sunday. */
  weekday: number;
  /** The local hour, 0–23. */
  hour: number;
  mode: ScanMode;
}

/** When the saved schedule next scans and last did, as Unix ms. */
export interface ScheduleStatus {
  nextMs: number | null;
  lastScanMs: number | null;
}

const preferenceSources = ["saved", "default", "invalid", "unavailable"] as const;
export type PreferenceSource = (typeof preferenceSources)[number];

export interface LoadedPreferences {
  preferences: Preferences;
  source: PreferenceSource;
}

const loginItems = ["enabled", "disabled", "requiresApproval", "unavailable", "unsupported"] as const;
export type LoginItem = (typeof loginItems)[number];

const notificationAccesses = ["allowed", "denied", "notAsked", "unavailable"] as const;
export type NotificationAccess = (typeof notificationAccesses)[number];

export const folderSources = ["protected", "cleanup", "developer", "restore"] as const;
export type FolderSource = (typeof folderSources)[number];

export interface AccessFolder {
  source: FolderSource;
  name: string;
  path: string;
}

export interface SettingsState {
  preferences: LoadedPreferences;
  loginItem: LoginItem;
  notifications: NotificationAccess;
  fullDiskAccess: boolean;
  folders: AccessFolder[];
  exclusions: string[];
  quarantineFolder: string;
  version: string;
  schedule: ScheduleStatus;
}

const MAX_TEXT = 1024;
const MAX_FOLDERS = 256;

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
  if (!Array.isArray(value) || value.length > MAX_FOLDERS) {
    throw new ContractError(field);
  }
  return value;
}

function choice<T extends number>(allowed: readonly T[], value: unknown, field: string): T {
  const found = allowed.find((entry) => entry === value);
  if (found === undefined) {
    throw new ContractError(field);
  }
  return found;
}

function scanOptions(value: unknown, field: string): ScanOptionsInput {
  const fields = record(value, field);
  return {
    logs: boolean(fields.logs, `${field}.logs`),
    temp: boolean(fields.temp, `${field}.temp`),
    logAgeDays: choice([7, 30, 90] as const, fields.logAgeDays, `${field}.logAgeDays`),
    largeMinMb: choice([500, 1000, 5000] as const, fields.largeMinMb, `${field}.largeMinMb`),
    oldDays: choice([90, 180, 365] as const, fields.oldDays, `${field}.oldDays`),
    hidden: boolean(fields.hidden, `${field}.hidden`),
    xcode: boolean(fields.xcode, `${field}.xcode`),
    sim: boolean(fields.sim, `${field}.sim`),
    node: boolean(fields.node, `${field}.node`),
    docker: boolean(fields.docker, `${field}.docker`),
  };
}

function preferences(value: unknown, field: string): Preferences {
  const fields = record(value, field);
  return {
    defaultScan: oneOf(scanModes, fields.defaultScan, `${field}.defaultScan`),
    scanDefaults: scanOptions(fields.scanDefaults, `${field}.scanDefaults`),
    minSizeMb: nonNegativeInteger(fields.minSizeMb, `${field}.minSizeMb`),
    skipRecentDays: nonNegativeInteger(fields.skipRecentDays, `${field}.skipRecentDays`),
    quarantineReminderGb: nonNegativeInteger(fields.quarantineReminderGb, `${field}.quarantineReminderGb`),
    notifyScan: boolean(fields.notifyScan, `${field}.notifyScan`),
    notifyQuarantine: boolean(fields.notifyQuarantine, `${field}.notifyQuarantine`),
    notifySound: boolean(fields.notifySound, `${field}.notifySound`),
    retentionDays: nonNegativeInteger(fields.retentionDays, `${field}.retentionDays`),
    menuBar: boolean(fields.menuBar, `${field}.menuBar`),
    checkForUpdates: boolean(fields.checkForUpdates, `${field}.checkForUpdates`),
    betaUpdates: fields.betaUpdates === null ? null : boolean(fields.betaUpdates, `${field}.betaUpdates`),
    scanSchedule: scanSchedule(fields.scanSchedule, `${field}.scanSchedule`),
  };
}

function bounded(value: unknown, max: number, field: string): number {
  const number = nonNegativeInteger(value, field);
  if (number > max) {
    throw new ContractError(field);
  }
  return number;
}

function scanSchedule(value: unknown, field: string): ScanSchedule {
  const fields = record(value, field);
  return {
    frequency: oneOf(scheduleFrequencies, fields.frequency, `${field}.frequency`),
    weekday: bounded(fields.weekday, 6, `${field}.weekday`),
    hour: bounded(fields.hour, 23, `${field}.hour`),
    mode: oneOf(scanModes, fields.mode, `${field}.mode`),
  };
}

function optionalTime(value: unknown, field: string): number | null {
  return value === null ? null : nonNegativeInteger(value, field);
}

function scheduleStatus(value: unknown, field: string): ScheduleStatus {
  const fields = record(value, field);
  return { nextMs: optionalTime(fields.nextMs, `${field}.nextMs`), lastScanMs: optionalTime(fields.lastScanMs, `${field}.lastScanMs`) };
}

export function parseLoadedPreferences(payload: unknown, field = "$"): LoadedPreferences {
  const fields = record(payload, field);
  return {
    preferences: preferences(fields.preferences, `${field}.preferences`),
    source: oneOf(preferenceSources, fields.source, `${field}.source`),
  };
}

/** Turning Open at login on or off: what macOS reports now, and its reason if it refused. */
export interface LoginChange {
  state: LoginItem;
  refusal: string | null;
}

export function parseLoginChange(payload: unknown): LoginChange {
  const fields = record(payload, "$");
  return {
    state: oneOf(loginItems, fields.state, "state"),
    refusal: fields.refusal === null ? null : text(fields.refusal, "refusal"),
  };
}

export function parseNotificationAccess(payload: unknown): NotificationAccess {
  return oneOf(notificationAccesses, payload, "$");
}

export function parseFolders(payload: unknown, field = "$"): AccessFolder[] {
  return list(payload, field).map((entry, index) => {
    const folder = record(entry, `${field}[${index}]`);
    return {
      source: oneOf(folderSources, folder.source, `${field}[${index}].source`),
      name: text(folder.name, `${field}[${index}].name`),
      path: text(folder.path, `${field}[${index}].path`),
    };
  });
}

export function parsePaths(payload: unknown, field = "$"): string[] {
  return list(payload, field).map((entry, index) => text(entry, `${field}[${index}]`));
}

/** What an uninstall moved to the Trash, and what it couldn't, as `~/…`. */
export interface Uninstalled {
  moved: string[];
  failed: string[];
  /** Open at login was on and macOS didn't turn it off: its reason. */
  loginItemKept: string | null;
}

export function parseUninstalled(payload: unknown): Uninstalled {
  const fields = record(payload, "$");
  return {
    moved: parsePaths(fields.moved, "moved"),
    failed: parsePaths(fields.failed, "failed"),
    loginItemKept: fields.loginItemKept === null ? null : text(fields.loginItemKept, "loginItemKept"),
  };
}

/** `null` when the folder panel was cancelled. */
export function parseOptionalPaths(payload: unknown): string[] | null {
  return payload === null ? null : parsePaths(payload);
}

export function parseSettings(payload: unknown): SettingsState {
  const fields = record(payload, "$");
  return {
    preferences: parseLoadedPreferences(fields.preferences, "preferences"),
    loginItem: oneOf(loginItems, fields.loginItem, "loginItem"),
    notifications: oneOf(notificationAccesses, fields.notifications, "notifications"),
    fullDiskAccess: boolean(fields.fullDiskAccess, "fullDiskAccess"),
    folders: parseFolders(fields.folders, "folders"),
    exclusions: parsePaths(fields.exclusions, "exclusions"),
    quarantineFolder: text(fields.quarantineFolder, "quarantineFolder"),
    version: text(fields.version, "version"),
    schedule: scheduleStatus(fields.schedule, "schedule"),
  };
}
