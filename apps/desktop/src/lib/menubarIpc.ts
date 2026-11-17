import { hrefFor, type V2Route } from "../v2/shell/routes";
import { call } from "./storageIpc";

/** The pages the popover can open; mirrors `Destination` in src-tauri/src/menubar.rs.
 * Preview needs a selection made in the main window. `network` is Activity's Network tab. */
export type MenuBarDestination =
  | Extract<V2Route, "dashboard" | "cleanup" | "review" | "quarantine" | "storage" | "system" | "developer" | "apps" | "maintenance" | "processes" | "mouse" | "activity" | "settings">
  | "network";

/** Where a destination is in the main window's hash. */
export function menuBarHref(destination: MenuBarDestination): string {
  return destination === "network" ? hrefFor("activity", "network") : hrefFor(destination);
}

/** Closes the popover and brings the main window forward at `destination`. */
export async function openMain(destination: MenuBarDestination): Promise<void> {
  await call("open_main", { destination }, () => undefined);
}

export async function hidePopover(): Promise<void> {
  await call("hide_popover", {}, () => undefined);
}

/** Asks for the popover window to be `height` points tall (its content's height); the app
 * keeps it within the screen. */
export async function fitPopover(height: number): Promise<void> {
  await call("fit_popover", { height }, () => undefined);
}

/** Quits Kleari, menu-bar icon included. */
export async function quitKleari(): Promise<void> {
  await call("quit_kleari", {}, () => undefined);
}
