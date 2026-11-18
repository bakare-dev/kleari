import { parseAppsScan, parseRemoveOutcome, type AppsScan, type RemoveOutcome } from "../contracts/apps";
import { parsePreview, type CleanupPreview } from "../contracts/cleanup";
import { call } from "./storageIpc";

/** Lists installed apps, leftovers of apps that are gone and installers, read-only. */
export function startAppsScan(): Promise<AppsScan> {
  return call("start_apps_scan", {}, parseAppsScan);
}

export async function cancelAppsScan(): Promise<void> {
  await call("cancel_apps_scan", {}, () => undefined);
}

/** Prepares leftovers and installers for removal here: their preview (nothing moves yet). */
export function prepareAppItemsRemoval(ids: readonly string[]): Promise<CleanupPreview> {
  return call("prepare_app_items_removal", { ids }, parsePreview);
}

/** Prepares the installed apps' caches (only their folders in ~/Library/Caches) for removal
 * here: their preview. */
export function prepareAppCachesRemoval(ids: readonly string[]): Promise<CleanupPreview> {
  return call("prepare_app_caches_removal", { ids }, parsePreview);
}

/** Moves the app to the Trash through macOS and prepares its leftovers for removal. */
export function removeApp(id: string): Promise<RemoveOutcome> {
  return call("remove_app", { id }, parseRemoveOutcome);
}
