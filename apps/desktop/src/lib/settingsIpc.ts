import { parseActivity, type ActivityList } from "../contracts/activity";
import {
  parseFolders,
  parseLoadedPreferences,
  parseLoginChange,
  parseNotificationAccess,
  parseOptionalPaths,
  parsePaths,
  parseSettings,
  parseUninstalled,
  type AccessFolder,
  type FolderSource,
  type LoadedPreferences,
  type LoginChange,
  type NotificationAccess,
  type Preferences,
  type SettingsState,
  type Uninstalled,
} from "../contracts/settings";
import { call } from "./storageIpc";

export function getSettings(): Promise<SettingsState> {
  return call("get_settings", {}, parseSettings);
}

/** Saves every preference at once; Rust refuses values the page doesn't offer. */
export function setPreferences(preferences: Preferences): Promise<LoadedPreferences> {
  return call("set_preferences", { preferences }, parseLoadedPreferences);
}

export function setOpenAtLogin(enabled: boolean): Promise<LoginChange> {
  return call("set_open_at_login", { enabled }, parseLoginChange);
}

/** macOS asks the user only the first time; afterwards this reports the answer. */
export function requestNotifications(): Promise<NotificationAccess> {
  return call("request_notifications", {}, parseNotificationAccess);
}

/** Opens the macOS folder panel; `null` when it was cancelled. */
export function addExcludedFolder(): Promise<string[] | null> {
  return call("add_excluded_folder", {}, parseOptionalPaths);
}

export function removeExcludedFolder(path: string): Promise<string[]> {
  return call("remove_excluded_folder", { path }, (payload) => parsePaths(payload));
}

export function removeFolderAccess(source: FolderSource, path: string): Promise<AccessFolder[]> {
  return call("remove_folder_access", { source, path }, (payload) => parseFolders(payload));
}

export async function revealQuarantineFolder(): Promise<void> {
  await call("reveal_quarantine_folder", {}, () => undefined);
}

export function listActivity(): Promise<ActivityList> {
  return call("list_activity", {}, parseActivity);
}

/**
 * Tries to turn off Open at login (the result says if macOS kept it on) and moves Kleari and everything it keeps to the Trash. Refused
 * (`quarantineNotEmpty`) while quarantine holds anything. Quit afterwards.
 */
export function uninstallKleari(): Promise<Uninstalled> {
  return call("uninstall_kleari", {}, parseUninstalled);
}

export async function clearActivity(): Promise<void> {
  await call("clear_activity", {}, () => undefined);
}
