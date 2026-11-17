import { parsePreview, type CleanupPreview } from "../contracts/cleanup";
import { parseFilesScan, type FilesOptions, type FilesScan, type Tool } from "../contracts/files";
import { call } from "./storageIpc";

/** Runs one Files tool, read-only. */
export function startFilesScan(tool: Tool, options: FilesOptions): Promise<FilesScan> {
  return call("start_files_scan", { tool, options }, parseFilesScan);
}

export async function cancelFilesScan(): Promise<void> {
  await call("cancel_files_scan", {}, () => undefined);
}

/** Prepares chosen files for removal here (one of each group of copies is always kept): their
 * preview, nothing moves yet. */
export function prepareFilesRemoval(ids: readonly string[]): Promise<CleanupPreview> {
  return call("prepare_files_removal", { ids }, parsePreview);
}
