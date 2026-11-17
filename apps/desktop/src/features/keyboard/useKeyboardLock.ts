import { useCallback, useEffect, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { parseLockState, type LockDuration, type LockStart, type LockState } from "../../contracts/keyboardLock";
import { getKeyboardLock, LOCK_EVENT, startKeyboardLock } from "../../lib/keyboardLockIpc";
import { StorageError } from "../../lib/storageIpc";

/** Why the lock didn't start, in words; `null` when it did. */
export function lockStartText(start: LockStart): string | null {
  switch (start.kind) {
    case "locked":
      return null;
    case "notAllowed":
      return "Kleari needs Accessibility to hold the keys. Turn it on in System Settings, then try again.";
    case "busy":
      return "A shortcut is being recorded on the Mouse page. Try again once it ends.";
    case "unsupported":
      return "Keyboard cleaning works in the Kleari app on a Mac.";
  }
}

/**
 * Keyboard cleaning mode, held at the app level so the lock screen shows over any page. Read
 * at launch, then kept current by Rust's `keyboard-lock` events; nothing is polled.
 */
export function useKeyboardLock() {
  const [state, setState] = useState<LockState>({ state: "unlocked" });
  const [error, setError] = useState<string | null>(null);
  const [needsAccess, setNeedsAccess] = useState(false);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    let stop: (() => void) | undefined;
    let closed = false;
    listen<unknown>(LOCK_EVENT, (event) => {
      try {
        setState(parseLockState(event.payload));
      } catch {
        // Deliberate: a malformed event changes nothing shown; the next one, or a read, corrects it.
      }
    }).then(
      (unlisten) => {
        if (closed) {
          unlisten();
        } else {
          stop = unlisten;
        }
      },
      // Not listening: the read below still shows a lock running now.
      () => undefined,
    );
    getKeyboardLock().then(
      (now) => !closed && setState(now),
      // Unread, it shows as unlocked: the keyboard's own state is unaffected.
      () => undefined,
    );
    return () => {
      closed = true;
      stop?.();
    };
  }, []);

  const start = useCallback((seconds: LockDuration) => {
    setError(null);
    setNeedsAccess(false);
    startKeyboardLock(seconds).then(
      (started) => {
        if (started.kind === "locked") {
          setState({ state: "locked", endsAtMs: started.endsAtMs });
        } else {
          setError(lockStartText(started));
          setNeedsAccess(started.kind === "notAllowed");
        }
      },
      (failure: unknown) => setError(failure instanceof StorageError && failure.kind === "backendUnavailable" ? lockStartText({ kind: "unsupported" }) : "The keyboard couldn’t be locked. Try again."),
    );
  }, []);

  return { state, error, needsAccess, start };
}

export type KeyboardLock = ReturnType<typeof useKeyboardLock>;
