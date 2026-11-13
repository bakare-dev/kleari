import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  parseStartupItems,
  parseSystemActivity,
  parseSystemDetails,
  type StartupItems,
  type SystemActivity,
  type SystemDetails,
} from "../contracts/systemPage";
import { invokeValidated } from "./systemMetricsIpc";

/** Read-only live readings for the System page charts (about once a second). */
export function fetchSystemActivity(): Promise<SystemActivity> {
  return invokeValidated("get_system_activity", parseSystemActivity);
}

/** Read-only hardware and OS details. */
export function fetchSystemDetails(): Promise<SystemDetails> {
  return invokeValidated("get_system_details", parseSystemDetails);
}

/** Read-only list of launch agents and daemons. */
export function fetchStartupItems(): Promise<StartupItems> {
  return invokeValidated("list_startup_items", parseStartupItems);
}

export type SettingsPane = "softwareUpdate" | "loginItems" | "fullDiskAccess" | "filesAndFolders" | "notifications" | "accessibility" | "appManagement";

/** Asks macOS to open one fixed System Settings pane. Resolves to whether it opened. */
export async function openSystemSettings(pane: SettingsPane): Promise<boolean> {
  if (!isTauri()) {
    return false;
  }
  try {
    await invoke("open_system_settings", { pane });
    return true;
  } catch {
    // Reported to the caller as `false`, which the UI shows; there is nothing else to recover.
    return false;
  }
}
