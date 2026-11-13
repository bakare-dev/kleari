import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  applyAppearanceToDocument,
  browserStorage,
  loadAppearance,
  saveAppearance,
  windowThemeFor,
  type AppearancePreference,
  type AppearanceStorageStatus,
} from "./appearance";

const DARK_QUERY = "(prefers-color-scheme: dark)";

function subscribeToSystemTheme(onChange: () => void) {
  const query = window.matchMedia(DARK_QUERY);
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}

/** Whether macOS is currently in Dark Mode, for the "System" option's description. */
function useSystemIsDark(): boolean {
  return useSyncExternalStore(
    subscribeToSystemTheme,
    () => window.matchMedia(DARK_QUERY).matches,
    () => false,
  );
}

export function useAppearance() {
  const [initial] = useState(() => loadAppearance(browserStorage()));
  const [preference, setPreference] = useState<AppearancePreference>(initial.preference);
  const [storageStatus, setStorageStatus] = useState<AppearanceStorageStatus>(initial.status);
  const [windowThemeFailed, setWindowThemeFailed] = useState(false);
  const systemIsDark = useSystemIsDark();

  useEffect(() => {
    applyAppearanceToDocument(document.documentElement, preference);
    if (!isTauri()) {
      return;
    }
    // Keep the native title bar consistent with the page; a failure is shown in Settings.
    let cancelled = false;
    getCurrentWindow()
      .setTheme(windowThemeFor(preference))
      .then(
        () => !cancelled && setWindowThemeFailed(false),
        () => !cancelled && setWindowThemeFailed(true),
      );
    return () => {
      cancelled = true;
    };
  }, [preference]);

  const choose = useCallback((next: AppearancePreference) => {
    setPreference(next);
    setStorageStatus(saveAppearance(browserStorage(), next) ? "saved" : "saveFailed");
  }, []);

  return { preference, choose, storageStatus, windowThemeFailed, systemIsDark };
}

export type Appearance = ReturnType<typeof useAppearance>;
