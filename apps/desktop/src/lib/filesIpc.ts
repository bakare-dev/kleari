import { parseFilesScan, parseSent, type FilesOptions, type FilesScan, type Tool } from "../contracts/files";
import { call } from "./storageIpc";

/** Runs one Files tool, read-only. */
export function startFilesScan(tool: Tool, options: FilesOptions): Promise<FilesScan> {
  return call("start_files_scan", { tool, options }, parseFilesScan);
}

export async function cancelFilesScan(): Promise<void> {
  await call("cancel_files_scan", {}, () => undefined);
}

/** Sends chosen files to Cleanup's review; one of each group of copies is always kept. */
export function sendFilesToReview(ids: readonly string[]): Promise<string[]> {
  return call("send_files_to_review", { ids }, parseSent);
}
