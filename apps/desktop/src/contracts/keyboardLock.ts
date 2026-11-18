// IPC contract for keyboard cleaning mode. Mirrors src-tauri/src/mouse/keylock.rs.

import { ContractError, nonNegativeInteger, oneOf, record } from "./systemMetrics";

/** The lengths offered, in seconds (`keylock::DURATIONS`). */
export const lockDurations = [30, 60, 120, 300] as const;
export type LockDuration = (typeof lockDurations)[number];

export type LockState = { state: "locked"; endsAtMs: number } | { state: "unlocked"; reason?: "click" | "time" | "stopped" };

export type LockStart = { kind: "locked"; endsAtMs: number } | { kind: "notAllowed" | "busy" | "unsupported" };

/** `get_keyboard_lock`, and each `keyboard-lock` event. */
export function parseLockState(payload: unknown): LockState {
  const fields = record(payload, "$");
  if (fields.state === "locked") {
    return { state: "locked", endsAtMs: nonNegativeInteger(fields.endsAtMs, "endsAtMs") };
  }
  if (fields.state === "unlocked") {
    return fields.reason === undefined ? { state: "unlocked" } : { state: "unlocked", reason: oneOf(["click", "time", "stopped"] as const, fields.reason, "reason") };
  }
  throw new ContractError("state");
}

export function parseLockStart(payload: unknown): LockStart {
  const fields = record(payload, "$");
  if (fields.kind === "locked") {
    return { kind: "locked", endsAtMs: nonNegativeInteger(fields.endsAtMs, "endsAtMs") };
  }
  return { kind: oneOf(["notAllowed", "busy", "unsupported"] as const, fields.kind, "kind") };
}
