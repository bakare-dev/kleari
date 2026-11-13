// Turns the validated process list into what the Processes page shows. Pure: live, paused,
// waiting, stale and unavailable are decided here, and CPU history is only what was sampled
// while the page was open.

import type { ProcessList, ProcessRow } from "../../contracts/processes";
import { errorText } from "../overview/format";
import { toneFor, type Loadable } from "../system/systemView";
import type { ProcessSample } from "../../v2/screens/processes/ProcessesScreen";
import type { LiveState } from "../../v2/ui/components";
import { plural } from "../../v2/ui/format";

export const PROCESS_INTERVAL_MS = 2_000;
/** 30 seconds of history at the polling interval. */
export const HISTORY_SLOTS = 15;

const GIB = 1024 ** 3;
const DAY_MS = 86_400_000;

export interface CpuHistory {
  /** When the last sample was taken; a longer gap than the limit starts every line fresh. */
  sampledAtMs: number | null;
  byId: ReadonlyMap<string, readonly (number | null)[]>;
}

export const emptyHistory: CpuHistory = { sampledAtMs: null, byId: new Map() };

/**
 * Adds one sample. A row without a CPU reading adds a gap, never a value; rows that are gone
 * are dropped; a gap longer than `maxGapMs` (pause, hidden window) clears every line.
 */
export function appendCpuHistory(history: CpuHistory, list: ProcessList, maxGapMs: number): CpuHistory {
  const fresh = history.sampledAtMs === null || list.sampledAtMs - history.sampledAtMs > maxGapMs;
  const byId = new Map<string, readonly (number | null)[]>();
  for (const row of list.rows) {
    const previous = fresh ? [] : (history.byId.get(row.id) ?? []);
    byId.set(row.id, [...previous, row.cpuPercent].slice(-HISTORY_SLOTS));
  }
  return { sampledAtMs: list.sampledAtMs, byId };
}

function latest<T>(state: Loadable<T>): T | null {
  return state.status === "ready" ? state.value : state.status === "error" ? state.last : null;
}

export function processLiveState(state: Loadable<ProcessList>, paused: boolean): LiveState {
  if (state.status === "error") {
    return state.last ? "stale" : "unavailable";
  }
  if (paused) {
    return "paused";
  }
  // The first sample (or the first after a pause) has no CPU window yet.
  return state.status === "ready" && state.value.windowMs !== null ? "live" : "waiting";
}

export function processLiveNote(state: Loadable<ProcessList>): string | null {
  if (state.status !== "error") {
    return null;
  }
  return state.last ? "The latest refresh failed. The list below is from the last successful one." : errorText[state.error];
}

export function formatStarted(startedAtMs: number | null, nowMs: number): string {
  if (startedAtMs === null) {
    return "Not reported";
  }
  const started = new Date(startedAtMs);
  const time = started.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  const startOfToday = new Date(nowMs);
  startOfToday.setHours(0, 0, 0, 0);
  const daysAgo = Math.floor((startOfToday.getTime() - startedAtMs) / DAY_MS) + 1;
  if (startedAtMs >= startOfToday.getTime()) {
    return `Today ${time}`;
  }
  if (daysAgo <= 1) {
    return `Yesterday ${time}`;
  }
  return daysAgo < 7 ? `${daysAgo} days ago` : started.toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

const kindSub = { background: "Background process", system: "System process" } as const;

export function toSample(row: ProcessRow, history: readonly (number | null)[], nowMs: number): ProcessSample {
  return {
    id: row.id,
    name: row.name,
    sub: row.kind === "app" ? plural(row.processCount, "process", "processes") : (row.location ?? kindSub[row.kind]),
    kind: row.kind,
    cpu: row.cpuPercent,
    cpuLowerBound: row.cpuPercent !== null && row.cpuMeasuredCount < row.processCount,
    cpuHistory: history,
    // Memory in binary units, as macOS and the Memory card show it.
    memGb: row.memoryBytes / GIB,
    pid: row.pid,
    processCount: row.processCount,
    user: row.user ?? "Not reported",
    started: formatStarted(row.startedAtMs, nowMs),
    threads: row.threads === null ? "Not available" : String(row.threads),
    path: row.location ?? "Not reported",
    tone: toneFor(row.name),
    isSelf: row.isSelf,
    canQuit: row.canQuit,
  };
}

export interface ProcessesView {
  processes: ProcessSample[];
  runningTotal: number;
  unreadableCount: number;
  cores: number;
  totalCpu: number | null;
  memoryUsedGb: number | null;
  memoryTotalGb: number | null;
  live: LiveState;
  liveNote: string | null;
  historySlots: number;
}

export function buildProcessesView(input: { state: Loadable<ProcessList>; history: CpuHistory; paused: boolean; nowMs: number }): ProcessesView {
  const list = latest(input.state);
  return {
    processes: list ? list.rows.map((row) => toSample(row, input.history.byId.get(row.id) ?? [], input.nowMs)) : [],
    runningTotal: list?.totalCount ?? 0,
    unreadableCount: list?.unreadableCount ?? 0,
    cores: list?.logicalCpus ?? 0,
    totalCpu: list?.totalCpuPercent ?? null,
    memoryUsedGb: list ? list.memoryUsedBytes / GIB : null,
    memoryTotalGb: list ? list.memoryTotalBytes / GIB : null,
    live: processLiveState(input.state, input.paused),
    liveNote: processLiveNote(input.state),
    historySlots: HISTORY_SLOTS,
  };
}
