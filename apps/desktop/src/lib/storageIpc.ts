import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import {
  parseFolderChoice,
  parseProgress,
  parseQueryResult,
  parseScanSummary,
  StorageContractError,
  type FileFilter,
  type FolderChoice,
  type QueryResult,
  type ScanProgress,
  type ScanSummary,
} from "../contracts/storage";
import {
  parseAnalysisProgress,
  parseStorageAnalysis,
  parseStorageOverview,
  type AnalysisProgress,
  type StorageAnalysisResult,
  type StorageOverview,
} from "../contracts/storageAnalysis";
import { ContractError } from "../contracts/systemMetrics";
import { invokeValidated } from "./systemMetricsIpc";

const backendCodes = [
  "pickerUnavailable",
  "grantNotFound",
  "rootChanged",
  "rootUnreadable",
  "scanInProgress",
  "noScanResult",
  "invalidFilter",
  "analysisInProgress",
  "analysisFailed",
  "developerScanInProgress",
  "developerScanFailed",
  "invalidRequest",
  "cleanupUnavailable",
  "cleanupScanInProgress",
  "cleanupScanFailed",
  "operationInProgress",
  "historyUnavailable",
  "revealFailed",
  "menuBarUnavailable",
  "quarantineNotEmpty",
  "uninstallFailed",
  "uninstallNeedsApp",
  "updateFailed",
  "updateNeedsApp",
  "mouseUnavailable",
  "appsScanInProgress",
  "appsScanFailed",
  "filesScanInProgress",
  "filesScanFailed",
] as const;

export type StorageErrorKind = (typeof backendCodes)[number] | "backendUnavailable" | "invalidResponse" | "unknown";

export class StorageError extends Error {
  readonly kind: StorageErrorKind;

  constructor(kind: StorageErrorKind, options?: { cause?: unknown }) {
    super(`Storage request failed: ${kind}`, options);
    this.name = "StorageError";
    this.kind = kind;
  }
}

function backendKind(error: unknown): StorageErrorKind {
  if (typeof error === "object" && error !== null && "code" in error) {
    const code = backendCodes.find((candidate) => candidate === error.code);
    if (code !== undefined) {
      return code;
    }
  }
  return "unknown";
}

/**
 * Invokes a command with arguments and validates the response, mapping failures to
 * `StorageErrorKind`. Shared by the Storage and Developer pages.
 */
export async function call<T>(command: string, args: Record<string, unknown>, parse: (payload: unknown) => T): Promise<T> {
  if (!isTauri()) {
    throw new StorageError("backendUnavailable");
  }
  let payload: unknown;
  try {
    payload = await invoke<unknown>(command, args);
  } catch (error) {
    throw new StorageError(backendKind(error), { cause: error });
  }
  try {
    return parse(payload);
  } catch (error) {
    if (error instanceof StorageContractError || error instanceof ContractError) {
      throw new StorageError("invalidResponse", { cause: error });
    }
    throw error;
  }
}

/** Startup-volume capacity, the space macOS manages, and whether Full Disk Access is on. */
export function fetchStorageOverview(): Promise<StorageOverview> {
  return invokeValidated("get_storage_overview", parseStorageOverview);
}

/**
 * Measures the startup disk's fixed locations read-only. The locations are chosen in Rust;
 * nothing here names a path. An invalid progress update is dropped, never shown.
 */
export function startStorageAnalysis(onProgress: (progress: AnalysisProgress) => void): Promise<StorageAnalysisResult> {
  const channel = new Channel<unknown>();
  channel.onmessage = (message) => {
    try {
      onProgress(parseAnalysisProgress(message));
    } catch (error) {
      if (!(error instanceof ContractError)) {
        throw error;
      }
      // Deliberate: an invalid progress update is ignored; the final result is validated in full.
    }
  };
  return call("start_storage_analysis", { onProgress: channel }, parseStorageAnalysis);
}

export async function cancelStorageAnalysis(): Promise<void> {
  await call("cancel_storage_analysis", {}, () => undefined);
}

/** Opens the native folder panel; the backend keeps the folder and returns only a grant ID. */
export function chooseScanFolder(): Promise<FolderChoice> {
  return call("choose_scan_folder", {}, parseFolderChoice);
}

/**
 * Scans a granted folder read-only. Progress messages are validated; a malformed one is
 * dropped rather than shown (the final summary is still validated in full).
 */
export function startScan(grantId: string, onProgress: (progress: ScanProgress) => void): Promise<ScanSummary> {
  const channel = new Channel<unknown>();
  channel.onmessage = (message) => {
    try {
      onProgress(parseProgress(message));
    } catch (error) {
      if (!(error instanceof StorageContractError)) {
        throw error;
      }
      // Deliberate: an invalid progress update is ignored; it never reaches the screen.
    }
  };
  return call("start_scan", { grantId, onProgress: channel }, parseScanSummary);
}

export async function cancelScan(grantId: string): Promise<void> {
  await call("cancel_scan", { grantId }, () => undefined);
}

export function queryScan(grantId: string, filter: FileFilter): Promise<QueryResult> {
  return call("query_scan", { grantId, filter }, (payload) => parseQueryResult(payload));
}
