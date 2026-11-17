import { parseLockStart, parseLockState, type LockDuration, type LockStart, type LockState } from "../contracts/keyboardLock";
import { call } from "./storageIpc";

/** Sent by Rust when the keyboard locks and when it unlocks. */
export const LOCK_EVENT = "keyboard-lock";

/** Locks the keyboard for `seconds`, until then or the first click; the main window comes forward. */
export function startKeyboardLock(seconds: LockDuration): Promise<LockStart> {
  return call("start_keyboard_lock", { seconds }, parseLockStart);
}

export function getKeyboardLock(): Promise<LockState> {
  return call("get_keyboard_lock", {}, parseLockState);
}
