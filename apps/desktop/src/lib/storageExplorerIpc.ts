import { Channel } from "@tauri-apps/api/core";
import { parseIds, parseListing, parseListingProgress, type Listing, type ListingProgress } from "../contracts/storageExplorer";
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

/** Sends chosen files and folders to Cleanup's review; nothing moves. Resolves to the IDs sent. */
export function sendStorageItemsToReview(ids: readonly string[]): Promise<string[]> {
  return call("send_storage_items_to_review", { ids }, parseIds);
}
