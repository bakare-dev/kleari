import { usePopover } from "../features/menubar/usePopover";
import { fitPopover } from "../lib/menubarIpc";
import { MenuBarPopover } from "./screens/popover/MenuBarPopover";
import "./v2.css";

/** Fits the window to the panel. */
function fit(height: number) {
  fitPopover(height).catch(() => {
    // Deliberate: if the window can't be resized it keeps its size, and the cards scroll.
  });
}

/** The menu-bar popover window (label "popover"), on live data in every build. */
export function PopoverApp() {
  const popover = usePopover();
  return <MenuBarPopover {...popover.data} failed={popover.failed} onGo={popover.go} onQuit={popover.quit} onFit={fit} />;
}
