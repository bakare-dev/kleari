import { useCallback, useEffect, useRef, useState } from "react";
import type { MouseStatus } from "../../contracts/mouse";
import { MouseSaves, type MouseChange } from "./mouseSaves";
import { getMouse, requestMouseAccess, setMouse } from "../../lib/mouseIpc";
import { errorKind } from "../cleanup/useCleanupScan";
import { storageErrorText } from "../storage/format";

export interface MouseLoaders {
  get: typeof getMouse;
  set: typeof setMouse;
  request: typeof requestMouseAccess;
}

const defaultLoaders: MouseLoaders = { get: getMouse, set: setMouse, request: requestMouseAccess };

/** How often the page reads again while waiting for Accessibility to be turned on. */
export const ACCESS_POLL_MS = 2_000;

/**
 * The Mouse settings, kept by the app so a save (and the combination changes waiting their
 * turn, `useMouseCombos`) finishes after leaving the Mouse page. A change shows at once; if it
 * can't be saved, it drops out with the reason (see `MouseSaves`). `watching` is whether the
 * Mouse page is open: it is read on each visit, and only then while waiting for Accessibility.
 */
export function useMouse(watching: boolean, loaders: MouseLoaders = defaultLoaders) {
  const [status, setStatus] = useState<MouseStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const [saves] = useState(
    () =>
      new MouseSaves(
        (next) => loaders.set(next),
        (failure) => storageErrorText[errorKind(failure)],
        (shown) => mounted.current && setStatus(shown),
      ),
  );

  // Reads go in turn with the saves (`MouseSaves.read`), so one never overwrites a later save.
  // One waiting read is enough: the poll and focus don't pile more up behind a slow save.
  const reading = useRef(false);
  const refresh = useCallback(() => {
    if (reading.current) {
      return;
    }
    reading.current = true;
    saves
      .read(() => loaders.get())
      .then(
        () => mounted.current && setError(null),
        (failure: unknown) => mounted.current && setError(storageErrorText[errorKind(failure)]),
      )
      .finally(() => {
        reading.current = false;
      });
  }, [loaders, saves]);

  useEffect(() => {
    if (watching) {
      refresh();
    }
  }, [watching, refresh]);

  // While the page is open, read again whenever the window comes back to the front: Accessibility
  // is turned on or off in System Settings, and the status should say so on return.
  useEffect(() => {
    if (!watching) {
      return;
    }
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [watching, refresh]);

  // Waiting on macOS (Accessibility turned on in System Settings, or the tap coming up): also
  // read again every few seconds.
  const waiting = watching && status !== null && status.access === "notAllowed";
  useEffect(() => {
    if (!waiting) {
      return;
    }
    const timer = setInterval(refresh, ACCESS_POLL_MS);
    return () => clearInterval(timer);
  }, [waiting, refresh]);

  /** Saves `change`; resolves with what to tell the user, or `null` once saved. */
  const save = useCallback((change: MouseChange): Promise<string | null> => saves.save(change), [saves]);

  /** Asks macOS for Accessibility access; macOS shows its own prompt the first time. */
  const requestAccess = useCallback(async (): Promise<string | null> => {
    try {
      await saves.read(() => loaders.request());
      if (mounted.current) {
        setError(null);
      }
      return null;
    } catch (failure) {
      return storageErrorText[errorKind(failure)];
    }
  }, [loaders, saves]);

  return { status, error, save, requestAccess, refresh };
}

export type Mouse = ReturnType<typeof useMouse>;
