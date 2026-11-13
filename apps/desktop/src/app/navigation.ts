import { useEffect } from "react";

/** ⌘, opens Settings, following the macOS convention (docs/design/INTERACTIONS.md). */
export function isSettingsShortcut(event: Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">) {
  return event.key === "," && event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
}

export function useSettingsShortcut() {
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (isSettingsShortcut(event)) {
        event.preventDefault();
        window.location.hash = "#settings";
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
