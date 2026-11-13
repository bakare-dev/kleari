import { usePopover } from "../features/menubar/usePopover";
import { MenuBarPopover } from "./screens/popover/MenuBarPopover";
import "./v2.css";

/** The menu-bar popover window (label "popover"), on live data in every build. */
export function PopoverApp() {
  const popover = usePopover();
  return <MenuBarPopover {...popover.data} failed={popover.failed} onGo={popover.go} onQuit={popover.quit} />;
}
