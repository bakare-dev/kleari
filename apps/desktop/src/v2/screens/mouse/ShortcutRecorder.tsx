import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { parseRecorded, type Shortcut } from "../../../contracts/mouse";
import { shortcutFrom, formatShortcut } from "../../../features/mouse/mouseView";
import { startShortcutRecording, stopShortcutRecording } from "../../../lib/mouseIpc";
import { Button } from "../../ui/components";

/** Sent by Rust for each key press while recording (src-tauri/src/mouse/recorder.rs). */
const RECORD_EVENT = "mouse-shortcut-record";

/**
 * Where recording happens: in macOS's keyboard tap (every shortcut, ⌘⇥ included, and the mouse
 * settings pause), or only inside this window when Accessibility isn't allowed (system
 * shortcuts then do their usual thing).
 */
type Mode = "starting" | "system" | "window";

/**
 * Records a shortcut: press it (again to change it), then Use it or Cancel. Nothing is saved
 * until Use. Recording ends on Use, Cancel, Escape, leaving the window, or after 30 seconds.
 */
export function ShortcutRecorder({ current, onRecord, onCancel }: { current: string | null; onRecord: (shortcut: Shortcut) => void; onCancel: () => void }) {
  const [recording, setRecording] = useState(current === null);
  const [mode, setMode] = useState<Mode>("starting");
  const [pending, setPending] = useState<Shortcut | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const field = useRef<HTMLButtonElement>(null);
  // The latest props, for a recording that ends by itself (Escape, time out, leaving the window).
  const latest = useRef({ current, onCancel });
  latest.current = { current, onCancel };

  const end = (cancelled: boolean) => {
    setRecording(false);
    setPending(null);
    setNote(null);
    if (cancelled && latest.current.current === null) {
      latest.current.onCancel();
    }
  };

  useEffect(() => {
    if (!recording) {
      return;
    }
    let closed = false;
    let unlisten: (() => void) | undefined;
    // The backend's number for this recording, once started: stopping names it, so a late stop
    // from here can never end a recording started after it.
    let session: number | null = null;
    setMode("starting");
    const inWindow = () => {
      setMode("window");
      setNote("System shortcuts like ⌘⇥ can be recorded once Kleari is allowed under Accessibility.");
      field.current?.focus();
    };
    if (!isTauri()) {
      inWindow();
      return;
    }
    listen(RECORD_EVENT, (event) => {
      let recorded;
      try {
        recorded = parseRecorded(event.payload);
      } catch {
        // A malformed report changes nothing; the next press is read as usual.
        return;
      }
      if (recorded.kind === "shortcut") {
        setPending(recorded.shortcut);
        setNote(null);
      } else if (recorded.kind === "unsupported") {
        setNote("Kleari can’t send that key. Try a letter, number, arrow, F-key, Tab, Space or Return.");
      } else {
        // Escape, or the time ran out.
        end(true);
      }
    })
      .then((stop) => {
        if (closed) {
          stop();
          return;
        }
        unlisten = stop;
        return startShortcutRecording();
      })
      .then((started) => {
        if (started === undefined) {
          return;
        }
        if (closed) {
          // Closed while it was starting: end the recording that just began.
          if (started.kind === "recording") {
            stopShortcutRecording(started.session).catch(() => undefined);
          }
          return;
        }
        if (started.kind === "recording") {
          session = started.session;
          setMode("system");
        } else {
          inWindow();
        }
      })
      .catch(() => {
        if (!closed) {
          inWindow();
        }
      });
    // Leaving the window lets the keyboard go: it must never stay captured while you work elsewhere.
    const leave = () => end(true);
    window.addEventListener("blur", leave);
    return () => {
      closed = true;
      window.removeEventListener("blur", leave);
      unlisten?.();
      // If this fails, recording still ends by itself after 30 seconds.
      if (session !== null) {
        stopShortcutRecording(session).catch(() => undefined);
      }
    };
    // Only `recording` starts and ends it: `end` sets state and reads the latest props from a
    // ref, so re-running on its identity would restart the recording on every render.
  }, [recording]);

  // Recording inside the window only: the focused field reads key presses.
  const onKeyDown = (event: KeyboardEvent) => {
    if (!recording || mode !== "window") {
      return;
    }
    event.preventDefault();
    if (event.key === "Escape" && !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey) {
      end(true);
      return;
    }
    const shortcut = shortcutFrom(event);
    if (shortcut === null) {
      setNote("Kleari can’t send that key. Try a letter, number, arrow, F-key, Tab, Space or Return.");
      return;
    }
    setPending(shortcut);
    setNote(null);
  };

  if (!recording) {
    return (
      <button type="button" className="mouse-shortcut" aria-label={`Shortcut ${current ?? ""}, change`} onClick={() => setRecording(true)}>
        {current}
      </button>
    );
  }

  return (
    <span className="mouse-recorder" role="group" aria-label="Record a shortcut">
      <button
        ref={field}
        type="button"
        className="mouse-shortcut recording"
        aria-live="polite"
        aria-label={pending ? `Recorded ${formatShortcut(pending)}. Press another to change it.` : "Press a shortcut, or Escape to cancel"}
        onKeyDown={onKeyDown}
      >
        {pending ? formatShortcut(pending) : mode === "starting" ? "…" : "Press a shortcut…"}
      </button>
      <Button
        variant="primary"
        size="sm"
        disabled={pending === null}
        onClick={() => {
          if (pending !== null) {
            const chosen = pending;
            end(false);
            onRecord(chosen);
          }
        }}
      >
        Use
      </Button>
      <Button variant="soft" size="sm" onClick={() => end(true)}>
        Cancel
      </Button>
      {mode === "system" && <span className="mouse-recorder-note">Mouse settings and macOS’s own shortcuts are paused while you record.</span>}
      {note && <span className="mouse-recorder-note">{note}</span>}
    </span>
  );
}
