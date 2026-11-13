import { parseMouseStatus, type MouseSettings, type MouseStatus } from "../contracts/mouse";
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
