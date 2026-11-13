import { useEffect, useRef } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** Sent by Rust when a scheduled scan has replaced the review (src-tauri/src/schedule.rs). */
export const SCHEDULED_SCAN_FINISHED = "scheduled-scan-finished";

/** Calls `onFinished` each time a scheduled scan finishes while this window is open. */
export function useScheduledScanFinished(onFinished: () => void) {
  const latest = useRef(onFinished);
  useEffect(() => {
    latest.current = onFinished;
  }, [onFinished]);

  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    let stop: (() => void) | undefined;
    let closed = false;
    listen(SCHEDULED_SCAN_FINISHED, () => latest.current()).then(
      (unlisten) => {
        if (closed) {
          unlisten();
        } else {
          stop = unlisten;
        }
      },
      // Not listening only means the review is read again on the next visit to it, as
      // every Cleanup screen does anyway.
      () => undefined,
    );
    return () => {
      closed = true;
      stop?.();
    };
  }, []);
}
