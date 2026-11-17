import { parsePreview, type CleanupPreview } from "../contracts/cleanup";
import { parseFilesScan, parseFoundFolder, type FilesOptions, type FilesScan, type Tool } from "../contracts/files";
import { call } from "./storageIpc";

/** Runs one Files tool, read-only. */
export function startFilesScan(tool: Tool, options: FilesOptions): Promise<FilesScan> {
  return call("start_files_scan", { tool, options }, parseFilesScan);
}

export async function cancelFilesScan(): Promise<void> {
  await call("cancel_files_scan", {}, () => undefined);
}

/** Opens a folder the last search found in the Browse view, read-only: its ID there (list it
 * with `listStorageFolder`), or `null` when it isn't known or changed since. */
export function openFoundFolder(id: string): Promise<string | null> {
  return call("open_found_folder", { id }, parseFoundFolder);
}

/** Prepares chosen files for removal here (one of each group of copies is always kept): their
 * preview, nothing moves yet. */
export function prepareFilesRemoval(ids: readonly string[]): Promise<CleanupPreview> {
  return call("prepare_files_removal", { ids }, parsePreview);
}
