import { parseMouseStatus, parseRecordingStart, type MouseSettings, type MouseStatus, type RecordingStart } from "../contracts/mouse";
import { call } from "./storageIpc";

/** The saved Mouse settings and whether they are in effect; also starts them once allowed. */
export function getMouse(): Promise<MouseStatus> {
  return call("get_mouse", {}, parseMouseStatus);
}

/** Saves and applies the settings at once; Rust refuses values the page doesn't offer. */
export function setMouse(settings: MouseSettings): Promise<MouseStatus> {
  return call("set_mouse", { settings }, parseMouseStatus);
}

/** Starts or stops the button finder reporting presses and scrolls in Kleari's window. */
export async function listenMouseProbe(on: boolean): Promise<void> {
  await call("listen_mouse_probe", { on }, () => undefined);
}

/** The pointer is over the button finder's box (sent again every second) or has left it. */
export async function captureMouseProbe(on: boolean): Promise<void> {
  await call("capture_mouse_probe", { on }, () => undefined);
}

/** Asks macOS for Accessibility access (its own prompt the first time). */
export function requestMouseAccess(): Promise<MouseStatus> {
  return call("request_mouse_access", {}, parseMouseStatus);
}

/** Starts recording a shortcut: every key press, system shortcuts included, goes to the page
 * (`mouse-shortcut-record`) and the mouse settings pause, until `stopShortcutRecording`. */
export function startShortcutRecording(): Promise<RecordingStart> {
  return call("start_shortcut_recording", {}, parseRecordingStart);
}

/** Ends a shortcut recording; the keyboard and the mouse settings work as usual again. */
export async function stopShortcutRecording(): Promise<void> {
  await call("stop_shortcut_recording", {}, () => undefined);
}
