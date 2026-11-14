import type { V2Route } from "../v2/shell/routes";
import { call } from "./storageIpc";

/** The screens the popover opens; Preview needs a selection made in the main window. */
/** The pages the popover can open; mirrors `Destination` in src-tauri/src/menubar.rs. */
export type MenuBarDestination = Extract<
  V2Route,
  "dashboard" | "cleanup" | "review" | "quarantine" | "storage" | "system" | "developer" | "apps" | "maintenance" | "processes" | "mouse" | "activity" | "settings"
>;

/** Closes the popover and brings the main window forward at `destination`. */
export async function openMain(destination: MenuBarDestination): Promise<void> {
  await call("open_main", { destination }, () => undefined);
}

export async function hidePopover(): Promise<void> {
  await call("hide_popover", {}, () => undefined);
}

/** Quits Kleari, menu-bar icon included. */
export async function quitKleari(): Promise<void> {
  await call("quit_kleari", {}, () => undefined);
}
