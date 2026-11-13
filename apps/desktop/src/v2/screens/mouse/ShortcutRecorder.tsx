import { useState, type KeyboardEvent } from "react";

/** Listens for the next key press on a focused button and makes it the shortcut. */
export function ShortcutRecorder({ current, onRecord, onCancel }: { current: string | null; onRecord: (event: KeyboardEvent) => void; onCancel: () => void }) {
  const [recording, setRecording] = useState(current === null);
  return (
    <button
      type="button"
      className={`mouse-shortcut${recording ? " recording" : ""}`}
      aria-label={recording ? "Press a shortcut, or Escape to cancel" : `Shortcut ${current ?? ""}, change`}
      autoFocus={current === null}
      onClick={() => setRecording(true)}
      onBlur={() => {
        setRecording(false);
        if (current === null) {
          onCancel();
        }
      }}
      onKeyDown={(event) => {
        if (!recording) {
          return;
        }
        event.preventDefault();
        if (event.key === "Escape" && !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey) {
          setRecording(false);
          if (current === null) {
            onCancel();
          }
          return;
        }
        onRecord(event);
      }}
    >
      {recording ? "Press a shortcut…" : current}
    </button>
  );
}
