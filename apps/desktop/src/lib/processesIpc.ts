import { invoke, isTauri } from "@tauri-apps/api/core";
import { parseProcessList, parseQuitOutcome, type ProcessList, type QuitOutcome } from "../contracts/processes";
import { invokeValidated } from "./systemMetricsIpc";

/** Read-only list of running processes for the Processes page. */
export function fetchProcessList(): Promise<ProcessList> {
  return invokeValidated("list_processes", parseProcessList);
}

/** `unavailable`: Kleari couldn't ask (no desktop app, or the request failed). */
export type QuitResult = QuitOutcome | "unavailable";

/**
 * Asks a regular app to quit, as choosing Quit from its menu does. `quitToken` is the row's
 * identity token; the backend refuses if the PID now belongs to another process.
 */
export async function quitApp(pid: number, quitToken: number): Promise<QuitResult> {
  if (!isTauri()) {
    return "unavailable";
  }
  let payload: unknown;
  try {
    payload = await invoke<unknown>("quit_app", { pid, quitToken });
  } catch {
    // Reported to the caller as `unavailable`, which the page shows; nothing else to recover.
    return "unavailable";
  }
  return parseQuitOutcome(payload);
}
