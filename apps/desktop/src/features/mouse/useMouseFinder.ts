import { useEffect, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { parseProbe } from "../../contracts/mouse";
import { captureMouseProbe, listenMouseProbe } from "../../lib/mouseIpc";
import { emptyFinder, FLASH_MS, finderStep, litParts, type Finder, type MousePart } from "./mouseView";

/** Sent by Rust for each press, release and scroll while the page listens. */
const PROBE_EVENT = "mouse-probe";

/** Where the buttons found on this Mac are remembered (this window's own storage). */
const SEEN_KEY = "kleari.mouse.seenButtons";

/** Remembered found buttons from `text`: button numbers 0–31, each once, in order; anything
 * else is ignored. */
export function parseSeen(text: string | null): number[] {
  if (text === null) {
    return [];
  }
  try {
    const value: unknown = JSON.parse(text);
    if (!Array.isArray(value)) {
      return [];
    }
    const buttons = value.filter((entry): entry is number => Number.isInteger(entry) && entry >= 0 && entry <= 31);
    return [...new Set(buttons)].sort((a, b) => a - b);
  } catch {
    // Unreadable: start afresh; the finder fills it in again.
    return [];
  }
}

function loadSeen(): number[] {
  try {
    return parseSeen(window.localStorage.getItem(SEEN_KEY));
  } catch {
    // Storage blocked: nothing remembered; the page still works.
    return [];
  }
}

function saveSeen(seen: readonly number[]) {
  try {
    window.localStorage.setItem(SEEN_KEY, JSON.stringify(seen));
  } catch {
    // Storage blocked: found buttons are forgotten when the page closes, nothing else changes.
  }
}

/** How often testing is renewed while the pointer stays over the box; Rust lets it lapse after
 * two seconds without one. */
export const CAPTURE_RENEW_MS = 1_000;

/**
 * Testing in the finder's box: while `over` is true, Kleari is told every second, so the middle
 * and side buttons, the wheel and right click only light up the drawing (and your mouse settings
 * pause there). Turned off when the pointer leaves, the window loses focus or the page closes;
 * if those are missed, Rust lets it lapse on its own.
 */
export function useFinderCapture(over: boolean) {
  useEffect(() => {
    if (!over || !isTauri()) {
      return;
    }
    // A missed renewal only lets testing lapse sooner; buttons then work as usual.
    const send = (on: boolean) => void captureMouseProbe(on).catch(() => undefined);
    send(true);
    const timer = setInterval(() => send(true), CAPTURE_RENEW_MS);
    return () => {
      clearInterval(timer);
      send(false);
    };
  }, [over]);
}

/**
 * The button finder while the Mouse page is open: Kleari reports which button was pressed
 * and how the wheel scrolled in its own window, and the drawn mouse lights the matching part.
 * `unavailable` when the report can't be turned on (outside the desktop app).
 */
export function useMouseFinder(): { finder: Finder; lit: Set<MousePart>; unavailable: boolean } {
  // Buttons found on earlier visits count as found, so the page can say which settings a
  // mouse can't trigger.
  const [finder, setFinder] = useState<Finder>(() => ({ ...emptyFinder, seen: loadSeen() }));
  useEffect(() => {
    saveSeen(finder.seen);
  }, [finder.seen]);
  // Re-rendered when a flash ends, so the part goes dark again.
  const [now, setNow] = useState(() => Date.now());
  const [unavailable, setUnavailable] = useState(!isTauri());

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    let stop: (() => void) | undefined;
    let closed = false;
    listen(PROBE_EVENT, (event) => {
      let probe;
      try {
        probe = parseProbe(event.payload);
      } catch {
        // A malformed report lights nothing; the next one is read as usual.
        return;
      }
      const at = Date.now();
      setFinder((current) => finderStep(current, probe, at));
      setNow(at);
    }).then(
      (unlisten) => {
        if (closed) {
          unlisten();
        } else {
          stop = unlisten;
        }
      },
      () => setUnavailable(true),
    );
    listenMouseProbe(true).catch(() => setUnavailable(true));
    return () => {
      closed = true;
      stop?.();
      // Leaving the page: the monitor goes quiet. If this fails it reports to no one.
      listenMouseProbe(false).catch(() => undefined);
    };
  }, []);

  // Wake once the newest flash is over, to turn it off.
  const nextEnd = finder.flashes.reduce((latest, flash) => Math.max(latest, flash.until), 0);
  useEffect(() => {
    if (nextEnd <= now) {
      return;
    }
    const timer = setTimeout(() => setNow(Date.now()), Math.min(nextEnd - now, FLASH_MS) + 10);
    return () => clearTimeout(timer);
  }, [nextEnd, now]);

  return { finder, lit: litParts(finder, now), unavailable };
}
