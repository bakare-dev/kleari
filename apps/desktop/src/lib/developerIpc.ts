import { Channel } from "@tauri-apps/api/core";
import {
  parseAddedFolder,
  parseDeveloperProgress,
  parseDeveloperScan,
  type AddedFolder,
  type DeveloperProgress,
  type DeveloperScan,
} from "../contracts/developer";
import { ContractError } from "../contracts/systemMetrics";
import { call } from "./storageIpc";

/**
 * Measures developer caches, project build folders and Docker's disk use, read-only. An
 * invalid progress update is dropped, never shown; the final result is validated in full.
 */
export function startDeveloperScan(onProgress: (progress: DeveloperProgress) => void): Promise<DeveloperScan> {
  const channel = new Channel<unknown>();
  channel.onmessage = (message) => {
    try {
      onProgress(parseDeveloperProgress(message));
    } catch (error) {
      if (!(error instanceof ContractError)) {
        throw error;
      }
      // Deliberate: an invalid progress update is ignored.
    }
  };
  return call("start_developer_scan", { onProgress: channel }, parseDeveloperScan);
}

export async function cancelDeveloperScan(): Promise<void> {
  await call("cancel_developer_scan", {}, () => undefined);
}

/** Opens the macOS folder panel; the backend keeps the folder and returns only its name. */
export function addDeveloperFolder(): Promise<AddedFolder> {
  return call("add_developer_folder", {}, parseAddedFolder);
}
