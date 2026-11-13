import { Channel } from "@tauri-apps/api/core";
import {
  parseAccess,
  parseAddedCleanupFolder,
  parseApproveOutcome,
  parseBoolean,
  parseCleanupProgress,
  parseCleanupScan,
  parseDeleteProgress,
  parseDeleteSummary,
  parseIdList,
  parseMoveProgress,
  parsePreview,
  parseQuarantine,
  parseRestoreResults,
  parseReview,
  parseScope,
  type AddedCleanupFolder,
  type ApproveOutcome,
  type CleanupPreview,
  type CleanupProgress,
  type CleanupScanResult,
  type DeleteProgress,
  type DeleteSummary,
  type FolderAccess,
  type MoveProgress,
  type ProtectedFolder,
  type QuarantineList,
  type RestoreResult,
  type ReviewList,
  type ScanMode,
  type ScanOptionsInput,
  type ScopeFolder,
} from "../contracts/cleanup";
import { ContractError } from "../contracts/systemMetrics";
import { call } from "./storageIpc";

/** A channel whose invalid messages are dropped, never shown; results are validated in full. */
function channel<T>(parse: (payload: unknown) => T, deliver: (message: T) => void): Channel<unknown> {
  const opened = new Channel<unknown>();
  opened.onmessage = (message) => {
    try {
      deliver(parse(message));
    } catch (error) {
      if (!(error instanceof ContractError)) {
        throw error;
      }
      // Deliberate: an invalid progress update is ignored.
    }
  };
  return opened;
}

export function getCleanupScope(): Promise<ScopeFolder[]> {
  return call("get_cleanup_scope", {}, parseScope);
}

/** Reads the folder once so macOS asks for access; waits while its prompt is open. */
export function requestFolderAccess(folder: ProtectedFolder): Promise<FolderAccess> {
  return call("request_folder_access", { folder }, parseAccess);
}

/** Opens the macOS folder panel; the backend keeps the folder. */
export function addCleanupFolder(): Promise<AddedCleanupFolder> {
  return call("add_cleanup_folder", {}, parseAddedCleanupFolder);
}

export function startCleanupScan(mode: ScanMode, options: ScanOptionsInput, onProgress: (progress: CleanupProgress) => void): Promise<CleanupScanResult> {
  return call("start_cleanup_scan", { mode, options, onProgress: channel(parseCleanupProgress, onProgress) }, parseCleanupScan);
}

export async function cancelCleanupScan(): Promise<void> {
  await call("cancel_cleanup_scan", {}, () => undefined);
}

export function getCleanupReview(): Promise<ReviewList> {
  return call("get_cleanup_review", {}, parseReview);
}

/** Returns the Developer review IDs the backend added to the review. */
export function sendDeveloperItemsToReview(ids: readonly string[]): Promise<string[]> {
  return call("send_developer_items_to_review", { ids }, parseIdList);
}

export function addStorageFileToReview(reviewId: string): Promise<boolean> {
  return call("add_storage_file_to_review", { reviewId }, parseBoolean);
}

export async function revealReviewItem(id: string): Promise<void> {
  await call("reveal_review_item", { id }, () => undefined);
}

export function createCleanupPreview(ids: readonly string[]): Promise<CleanupPreview> {
  return call("create_cleanup_preview", { ids }, parsePreview);
}

export function approveCleanupPreview(previewId: string, onProgress: (progress: MoveProgress) => void): Promise<ApproveOutcome> {
  return call("approve_cleanup_preview", { previewId, onProgress: channel(parseMoveProgress, onProgress) }, parseApproveOutcome);
}

export function listQuarantine(): Promise<QuarantineList> {
  return call("list_quarantine", {}, parseQuarantine);
}

export function restoreQuarantineItems(ids: readonly string[]): Promise<RestoreResult[]> {
  return call("restore_quarantine_items", { ids }, parseRestoreResults);
}

export function deleteQuarantineItems(ids: readonly string[], onProgress: (progress: DeleteProgress) => void): Promise<DeleteSummary> {
  return call("delete_quarantine_items", { ids, onProgress: channel(parseDeleteProgress, onProgress) }, parseDeleteSummary);
}

export async function stopQuarantineDelete(): Promise<void> {
  await call("stop_quarantine_delete", {}, () => undefined);
}

export async function revealQuarantineItem(id: string, withOriginal: boolean): Promise<void> {
  await call("reveal_quarantine_item", { id, withOriginal }, () => undefined);
}

/** Opens the macOS folder panel; items may be restored into the chosen folder this session
 * (it is never scanned). Unfinished restores are settled again afterwards. */
export function chooseRestoreFolder(): Promise<AddedCleanupFolder> {
  return call("choose_restore_folder", {}, parseAddedCleanupFolder);
}
