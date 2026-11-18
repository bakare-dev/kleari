import { useEffect, useState } from "react";
import type { LockState } from "../../contracts/keyboardLock";
import { IconTile } from "../ui/components";
import "./keyboardLock.css";

/** "1:05", "0:09". */
export function timeLeft(endsAtMs: number, nowMs: number): string {
  const seconds = Math.max(0, Math.ceil((endsAtMs - nowMs) / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

/** The time left, ticking once a second only while the lock screen is up. */
function Countdown({ endsAtMs }: { endsAtMs: number }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);
  return <span className="kl-time v2-num">{timeLeft(endsAtMs, now)}</span>;
}

/** Over the whole window while the keyboard is locked for cleaning. */
export function KeyboardLockScreen({ state }: { state: LockState }) {
  if (state.state !== "locked") {
    return null;
  }
  return (
    <div role="dialog" aria-modal="true" aria-labelledby="kl-title" aria-describedby="kl-text" className="kl-screen">
      <div className="kl-card">
        <IconTile tone="blue" icon="key" size={52} glow />
        <h2 id="kl-title" className="kl-title">Keyboard locked for cleaning</h2>
        <Countdown endsAtMs={state.endsAtMs} />
        <p id="kl-text" className="kl-text">Wipe away: no key does anything. Click anywhere, with the mouse or the trackpad, to unlock now.</p>
      </div>
    </div>
  );
}
