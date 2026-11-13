// IPC contracts for the Processes page. Mirrors src-tauri/src/system_monitor/{process_list,
// running_apps}.rs; every response is validated before the UI sees it.

import { ContractError, nonNegativeInteger, nullable, oneOf, record } from "./systemMetrics";

export type ProcessKind = "app" | "background" | "system";
export type QuitOutcome = "requested" | "notRunning" | "notAllowed" | "declined";

export interface ProcessRow {
  id: string;
  name: string;
  kind: ProcessKind;
  pid: number;
  processCount: number;
  /** Share of one CPU summed over members with a reading; `null` before any reading. */
  cpuPercent: number | null;
  /** Fewer than `processCount` makes `cpuPercent` a minimum. */
  cpuMeasuredCount: number;
  memoryBytes: number;
  user: string | null;
  startedAtMs: number | null;
  /** `null` when macOS doesn't let Kleari read it. */
  threads: number | null;
  location: string | null;
  isSelf: boolean;
  canQuit: boolean;
  /** Present exactly when `canQuit`: the identity `quit_app` checks before asking. */
  quitToken: number | null;
}

export interface ProcessList {
  sampledAtMs: number;
  /** `null` on the first sample or after a long gap: CPU is withheld. */
  windowMs: number | null;
  logicalCpus: number;
  totalCpuPercent: number | null;
  memoryTotalBytes: number;
  memoryUsedBytes: number;
  totalCount: number;
  unreadableCount: number;
  rows: ProcessRow[];
}

/** Mirrors `process_list::ROW_LIMIT`: the busiest and the largest, at most twice that. */
export const MAX_PROCESS_ROWS = 100;
const MAX_NAME = 200;
const MAX_LOCATION = 4096;

function text(value: unknown, field: string, max: number): string {
  if (typeof value !== "string" || value.length === 0 || value.length > max) {
    throw new ContractError(field);
  }
  return value;
}

function boolean(value: unknown, field: string): boolean {
  if (typeof value !== "boolean") {
    throw new ContractError(field);
  }
  return value;
}

function finiteNonNegative(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
    throw new ContractError(field);
  }
  return value;
}

const kinds: readonly ProcessKind[] = ["app", "background", "system"];
const outcomes: readonly QuitOutcome[] = ["requested", "notRunning", "notAllowed", "declined"];

function row(value: unknown, field: string, logicalCpus: number): ProcessRow {
  const fields = record(value, field);
  const processCount = nonNegativeInteger(fields.processCount, `${field}.processCount`);
  const cpuMeasuredCount = nonNegativeInteger(fields.cpuMeasuredCount, `${field}.cpuMeasuredCount`);
  const cpuPercent = nullable(fields.cpuPercent, (cpu) => finiteNonNegative(cpu, `${field}.cpuPercent`));
  // A reading needs a measured member, and a group can't use more than every CPU.
  if (
    processCount === 0 ||
    cpuMeasuredCount > processCount ||
    (cpuPercent === null) !== (cpuMeasuredCount === 0) ||
    (cpuPercent !== null && cpuPercent > Math.max(logicalCpus, 1) * 100)
  ) {
    throw new ContractError(`${field}.cpuPercent`);
  }
  const kind = oneOf(kinds, fields.kind, `${field}.kind`);
  const isSelf = boolean(fields.isSelf, `${field}.isSelf`);
  const canQuit = boolean(fields.canQuit, `${field}.canQuit`);
  // Only another regular app can be quit; never Kleari, a background or a system process.
  if (canQuit && (kind !== "app" || isSelf)) {
    throw new ContractError(`${field}.canQuit`);
  }
  const quitToken = nullable(fields.quitToken, (token) => nonNegativeInteger(token, `${field}.quitToken`));
  if ((quitToken !== null) !== canQuit || quitToken === 0) {
    throw new ContractError(`${field}.quitToken`);
  }
  return {
    id: text(fields.id, `${field}.id`, MAX_LOCATION),
    name: text(fields.name, `${field}.name`, MAX_NAME),
    kind,
    pid: nonNegativeInteger(fields.pid, `${field}.pid`),
    processCount,
    cpuPercent,
    cpuMeasuredCount,
    memoryBytes: nonNegativeInteger(fields.memoryBytes, `${field}.memoryBytes`),
    user: nullable(fields.user, (user) => text(user, `${field}.user`, MAX_NAME)),
    startedAtMs: nullable(fields.startedAtMs, (started) => nonNegativeInteger(started, `${field}.startedAtMs`)),
    threads: nullable(fields.threads, (threads) => nonNegativeInteger(threads, `${field}.threads`)),
    location: nullable(fields.location, (location) => text(location, `${field}.location`, MAX_LOCATION)),
    isSelf,
    canQuit,
    quitToken,
  };
}

export function parseProcessList(payload: unknown): ProcessList {
  const fields = record(payload, "$");
  if (!Array.isArray(fields.rows) || fields.rows.length > MAX_PROCESS_ROWS) {
    throw new ContractError("rows");
  }
  const logicalCpus = nonNegativeInteger(fields.logicalCpus, "logicalCpus");
  const memoryTotalBytes = nonNegativeInteger(fields.memoryTotalBytes, "memoryTotalBytes");
  const memoryUsedBytes = nonNegativeInteger(fields.memoryUsedBytes, "memoryUsedBytes");
  if (memoryUsedBytes > memoryTotalBytes) {
    throw new ContractError("memoryUsedBytes");
  }
  const totalCount = nonNegativeInteger(fields.totalCount, "totalCount");
  const unreadableCount = nonNegativeInteger(fields.unreadableCount, "unreadableCount");
  const rows = fields.rows.map((entry: unknown, index) => row(entry, `rows[${index}]`, logicalCpus));
  if (unreadableCount > totalCount || rows.length > totalCount) {
    throw new ContractError("totalCount");
  }
  const windowMs = nullable(fields.windowMs, (window) => nonNegativeInteger(window, "windowMs"));
  const totalCpuPercent = nullable(fields.totalCpuPercent, (cpu) => {
    const value = finiteNonNegative(cpu, "totalCpuPercent");
    if (value > 100) {
      throw new ContractError("totalCpuPercent");
    }
    return value;
  });
  return {
    sampledAtMs: nonNegativeInteger(fields.sampledAtMs, "sampledAtMs"),
    windowMs,
    logicalCpus,
    totalCpuPercent,
    memoryTotalBytes,
    memoryUsedBytes,
    totalCount,
    unreadableCount,
    rows,
  };
}

export function parseQuitOutcome(payload: unknown): QuitOutcome {
  return oneOf(outcomes, payload, "$");
}
