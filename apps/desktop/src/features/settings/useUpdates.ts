import { useCallback, useEffect, useRef, useState } from "react";
import type { SettingsState } from "../../contracts/settings";
import type { UpdateCheck } from "../../contracts/updates";
import { checkForUpdate, installUpdate } from "../../lib/updatesIpc";
import { errorKind } from "../cleanup/useCleanupScan";
import { storageErrorText } from "../storage/format";

export interface UpdateLoaders {
  check: typeof checkForUpdate;
  install: typeof installUpdate;
}

const defaultLoaders: UpdateLoaders = { check: checkForUpdate, install: installUpdate };

/**
 * Whether to check at launch: `null` until settings are read. Only a choice actually read
 * counts (saved, or no record yet so the default applies). If the saved settings can't be
 * read or used, Kleari can't know you didn't turn checks off, so it doesn't check.
 */
export function automaticUpdates(settings: SettingsState | null): boolean | null {
  if (settings === null) {
    return null;
  }
  const { preferences, source } = settings.preferences;
  return (source === "saved" || source === "default") && preferences.checkForUpdates;
}

export type UpdateState =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "checked"; check: UpdateCheck }
  | { phase: "installing"; check: UpdateCheck }
  | { phase: "failed"; message: string; check?: UpdateCheck };

/**
 * Checking for and installing updates. With `automatic` on (Settings › About, read from the
 * saved preferences; `null` until they are), it checks once at launch and offers an update it
 * finds; otherwise it checks only when asked. Installing always waits for you to choose it.
 */
export function useUpdates(automatic: boolean | null, loaders: UpdateLoaders = defaultLoaders) {
  const [state, setState] = useState<UpdateState>({ phase: "idle" });
  // The update offered at launch, until you install it or choose Later.
  const [offer, setOffer] = useState<UpdateCheck | null>(null);
  const launchChecked = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const check = useCallback(async (): Promise<UpdateCheck | null> => {
    setState({ phase: "checking" });
    try {
      const found = await loaders.check();
      if (mounted.current) {
        setState({ phase: "checked", check: found });
      }
      return found;
    } catch (failure) {
      if (mounted.current) {
        setState({ phase: "failed", message: storageErrorText[errorKind(failure)] });
      }
      return null;
    }
  }, [loaders]);

  useEffect(() => {
    if (automatic !== true || launchChecked.current) {
      return;
    }
    launchChecked.current = true;
    // A failed check at launch is shown in Settings › About only; it never interrupts.
    void check().then((found) => found?.status === "available" && mounted.current && setOffer(found));
  }, [automatic, check]);

  /** Installs what the last check found; Kleari restarts, so this resolves only on failure. */
  const install = useCallback(async () => {
    const found = state.phase === "checked" || state.phase === "failed" ? state.check : offer ?? undefined;
    if (!found || found.status !== "available") {
      return;
    }
    setState({ phase: "installing", check: found });
    try {
      await loaders.install();
    } catch (failure) {
      if (mounted.current) {
        setState({ phase: "failed", message: storageErrorText[errorKind(failure)], check: found });
      }
    }
  }, [loaders, offer, state]);

  const dismiss = useCallback(() => setOffer(null), []);

  return { state, offer, check, install, dismiss };
}

export type Updates = ReturnType<typeof useUpdates>;
