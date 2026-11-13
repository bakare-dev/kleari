import { parseUpdateCheck, type UpdateCheck } from "../contracts/updates";
import { call } from "./storageIpc";

/** Asks GitHub whether a newer Kleari is out. This is Kleari's only network request. */
export function checkForUpdate(): Promise<UpdateCheck> {
  return call("check_for_update", {}, parseUpdateCheck);
}

/** Installs the update the last check found and restarts; resolves only if it didn't. */
export async function installUpdate(): Promise<void> {
  await call("install_update", {}, () => undefined);
}
