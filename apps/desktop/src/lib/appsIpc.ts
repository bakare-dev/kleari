import { parseAppsScan, parseRemoveOutcome, parseSentIds, type AppsScan, type RemoveOutcome } from "../contracts/apps";
import { call } from "./storageIpc";

/** Lists installed apps, leftovers of apps that are gone and installers, read-only. */
export function startAppsScan(): Promise<AppsScan> {
  return call("start_apps_scan", {}, parseAppsScan);
}

export async function cancelAppsScan(): Promise<void> {
  await call("cancel_apps_scan", {}, () => undefined);
}

/** Sends leftovers and installers to Cleanup's review; nothing moves. Resolves to the IDs sent. */
export function sendAppItemsToReview(ids: readonly string[]): Promise<string[]> {
  return call("send_app_items_to_review", { ids }, parseSentIds);
}

/** Moves the app to the Trash through macOS and sends its leftovers to the review. */
export function removeApp(id: string): Promise<RemoveOutcome> {
  return call("remove_app", { id }, parseRemoveOutcome);
}
