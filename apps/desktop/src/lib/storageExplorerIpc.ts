import { Channel } from "@tauri-apps/api/core";
import { parsePreview, type CleanupPreview } from "../contracts/cleanup";
import { parseListing, parseListingProgress, type Listing, type ListingProgress } from "../contracts/storageExplorer";
import { ContractError } from "../contracts/systemMetrics";
import { call } from "./storageIpc";

/** Lists a folder (`null`: the home folder), read-only. `null` back: the ID was forgotten. */
export function listStorageFolder(id: string | null, onProgress: (progress: ListingProgress) => void): Promise<Listing | null> {
  const channel = new Channel<unknown>();
  channel.onmessage = (message) => {
    try {
      onProgress(parseListingProgress(message));
    } catch (error) {
      if (!(error instanceof ContractError)) {
        throw error;
      }
      // Deliberate: an invalid progress update is ignored.
    }
  };
  return call("list_storage_folder", { id, onProgress: channel }, parseListing);
}

export async function cancelStorageListing(): Promise<void> {
  await call("cancel_storage_listing", {}, () => undefined);
}

/** Prepares chosen files and folders for removal here: their preview (nothing moves yet). */
export function prepareStorageItemsRemoval(ids: readonly string[]): Promise<CleanupPreview> {
  return call("prepare_storage_items_removal", { ids }, parsePreview);
}
