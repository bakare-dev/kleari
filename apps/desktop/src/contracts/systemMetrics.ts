// Mirror of the Rust `system_monitor::SystemMetrics` DTO. Types describe the contract;
// `parseSystemMetrics` enforces it at runtime because IPC payloads are untrusted.

export type UnavailableReason = "warmingUp" | "notReported" | "notFound" | "notPresent";

export type Reading<T> =
  | { status: "available"; value: T }
  | { status: "unavailable"; reason: UnavailableReason };

export interface CpuReading {
  /** Average across all logical CPUs over `windowMs`, 0–100. */
  usagePercent: number;
  windowMs: number;
}

export interface MemoryReading {
  totalBytes: number;
  usedBytes: number;
  availableBytes: number;
}

export interface DiskReading {
  totalBytes: number;
  usedBytes: number;
  availableBytes: number;
}

export type PowerSource = "battery" | "ac";

export type ChargeState = "charging" | "charged" | "notCharging" | "discharging";

export interface BatteryReading {
  chargePercent: number;
  powerSource: PowerSource;
  state: ChargeState;
  /** Until full while charging, until empty while discharging; null while calculating. */
  minutesRemaining: number | null;
}

export interface ProcessReading {
  pid: number;
  name: string;
  /** Share of one logical CPU (can exceed 100); null until the process has two samples. */
  cpuPercent: number | null;
  memoryBytes: number;
}

/** Processes grouped by app bundle (or by name when outside any bundle). */
export interface AppReading {
  name: string;
  processCount: number;
  /** Sum over members with a CPU reading; null when no member has one yet. */
  cpuPercent: number | null;
  /** Members included in `cpuPercent`; fewer than `processCount` makes it a lower bound. */
  cpuMeasuredCount: number;
  /** Sum of members' resident memory; can count shared memory more than once. */
  memoryBytes: number;
}

export interface ProcessSummary {
  topByCpu: ProcessReading[];
  topByMemory: ProcessReading[];
  topAppsByCpu: AppReading[];
  topAppsByMemory: AppReading[];
  readableCount: number;
  /** Processes with no readable memory value, excluded from ranking (usually withheld by macOS). */
  unreadableCount: number;
  /** Null until a second sample exists, so CPU ranking is not yet available. */
  cpuWindowMs: number | null;
}

/** Mirrors `processes::TOP_COUNT` in Rust. */
export const TOP_PROCESS_COUNT = 5;

export interface SystemMetrics {
  sampledAtMs: number;
  source: string;
  cpu: Reading<CpuReading>;
  memory: Reading<MemoryReading>;
  disk: Reading<DiskReading>;
  battery: Reading<BatteryReading>;
  processes: Reading<ProcessSummary>;
}

export class ContractError extends Error {
  constructor(field: string) {
    super(`Invalid system metrics payload at "${field}"`);
    this.name = "ContractError";
  }
}

// The validators below are shared with the System page contracts (systemPage.ts).
export type Fields = Record<string, unknown>;

function isFields(value: unknown): value is Fields {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function record(value: unknown, field: string): Fields {
  if (!isFields(value)) {
    throw new ContractError(field);
  }
  return value;
}

export function nonNegativeInteger(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new ContractError(field);
  }
  return value;
}

export function percent(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0 || value > 100) {
    throw new ContractError(field);
  }
  return value;
}

export function oneOf<T extends string>(allowed: readonly T[], value: unknown, field: string): T {
  const match = allowed.find((candidate) => candidate === value);
  if (match === undefined) {
    throw new ContractError(field);
  }
  return match;
}

const unavailableReasons: readonly UnavailableReason[] = [
  "warmingUp",
  "notReported",
  "notFound",
  "notPresent",
];
const powerSources: readonly PowerSource[] = ["battery", "ac"];
const chargeStates: readonly ChargeState[] = ["charging", "charged", "notCharging", "discharging"];

export function unavailableReason(value: unknown, field: string): UnavailableReason {
  return oneOf(unavailableReasons, value, field);
}

export function battery(fields: Fields): BatteryReading {
  const minutes = fields.minutesRemaining;
  return {
    chargePercent: percent(fields.chargePercent, "battery.chargePercent"),
    powerSource: oneOf(powerSources, fields.powerSource, "battery.powerSource"),
    state: oneOf(chargeStates, fields.state, "battery.state"),
    minutesRemaining:
      minutes === null ? null : nonNegativeInteger(minutes, "battery.minutesRemaining"),
  };
}

export function nullable<T>(value: unknown, parse: (value: unknown) => T): T | null {
  return value === null ? null : parse(value);
}

function processReading(value: unknown, field: string): ProcessReading {
  const fields = record(value, field);
  if (typeof fields.name !== "string") {
    throw new ContractError(`${field}.name`);
  }
  return {
    pid: nonNegativeInteger(fields.pid, `${field}.pid`),
    name: fields.name,
    cpuPercent: cpuShare(fields.cpuPercent, `${field}.cpuPercent`),
    memoryBytes: nonNegativeInteger(fields.memoryBytes, `${field}.memoryBytes`),
  };
}

/** A share of one core, so values above 100 are valid; null when not yet measured. */
function cpuShare(value: unknown, field: string): number | null {
  return nullable(value, (cpu) => {
    if (typeof cpu !== "number" || !Number.isFinite(cpu) || cpu < 0) {
      throw new ContractError(field);
    }
    return cpu;
  });
}

function appReading(value: unknown, field: string): AppReading {
  const fields = record(value, field);
  if (typeof fields.name !== "string") {
    throw new ContractError(`${field}.name`);
  }
  const processCount = nonNegativeInteger(fields.processCount, `${field}.processCount`);
  if (processCount === 0) {
    throw new ContractError(`${field}.processCount`);
  }
  const cpuPercent = cpuShare(fields.cpuPercent, `${field}.cpuPercent`);
  const cpuMeasuredCount = nonNegativeInteger(fields.cpuMeasuredCount, `${field}.cpuMeasuredCount`);
  // A value needs at least one measured member; no value means none were measured.
  if (cpuMeasuredCount > processCount || (cpuPercent === null) !== (cpuMeasuredCount === 0)) {
    throw new ContractError(`${field}.cpuMeasuredCount`);
  }
  return {
    name: fields.name,
    processCount,
    cpuPercent,
    cpuMeasuredCount,
    memoryBytes: nonNegativeInteger(fields.memoryBytes, `${field}.memoryBytes`),
  };
}

function topList<T>(value: unknown, field: string, parse: (entry: unknown, field: string) => T): T[] {
  if (!Array.isArray(value) || value.length > TOP_PROCESS_COUNT) {
    throw new ContractError(field);
  }
  return value.map((entry, index) => parse(entry, `${field}[${index}]`));
}

function processSummary(fields: Fields): ProcessSummary {
  return {
    topByCpu: topList(fields.topByCpu, "processes.topByCpu", processReading),
    topByMemory: topList(fields.topByMemory, "processes.topByMemory", processReading),
    topAppsByCpu: topList(fields.topAppsByCpu, "processes.topAppsByCpu", appReading),
    topAppsByMemory: topList(fields.topAppsByMemory, "processes.topAppsByMemory", appReading),
    readableCount: nonNegativeInteger(fields.readableCount, "processes.readableCount"),
    unreadableCount: nonNegativeInteger(fields.unreadableCount, "processes.unreadableCount"),
    cpuWindowMs: nullable(fields.cpuWindowMs, (ms) =>
      nonNegativeInteger(ms, "processes.cpuWindowMs"),
    ),
  };
}

export function reading<T>(value: unknown, field: string, parseValue: (fields: Fields) => T): Reading<T> {
  return readingOf(value, field, (inner) => parseValue(record(inner, `${field}.value`)));
}

/** Like `reading`, for values that are not objects (a string or a number). */
export function readingOf<T>(value: unknown, field: string, parseValue: (value: unknown) => T): Reading<T> {
  const fields = record(value, field);
  if (fields.status === "available") {
    return { status: "available", value: parseValue(fields.value) };
  }
  if (fields.status === "unavailable") {
    return { status: "unavailable", reason: unavailableReason(fields.reason, `${field}.reason`) };
  }
  throw new ContractError(`${field}.status`);
}

export function capacity(fields: Fields, field: string): MemoryReading & DiskReading {
  const totalBytes = nonNegativeInteger(fields.totalBytes, `${field}.totalBytes`);
  const usedBytes = nonNegativeInteger(fields.usedBytes, `${field}.usedBytes`);
  const availableBytes = nonNegativeInteger(fields.availableBytes, `${field}.availableBytes`);
  if (totalBytes === 0 || usedBytes > totalBytes || availableBytes > totalBytes) {
    throw new ContractError(field);
  }
  return { totalBytes, usedBytes, availableBytes };
}

export function parseSystemMetrics(payload: unknown): SystemMetrics {
  const fields = record(payload, "$");
  const sampledAtMs = nonNegativeInteger(fields.sampledAtMs, "sampledAtMs");
  if (typeof fields.source !== "string" || fields.source.length === 0) {
    throw new ContractError("source");
  }
  return {
    sampledAtMs,
    source: fields.source,
    cpu: reading(fields.cpu, "cpu", (cpu) => ({
      usagePercent: percent(cpu.usagePercent, "cpu.usagePercent"),
      windowMs: nonNegativeInteger(cpu.windowMs, "cpu.windowMs"),
    })),
    memory: reading(fields.memory, "memory", (memory) => capacity(memory, "memory")),
    disk: reading(fields.disk, "disk", (disk) => capacity(disk, "disk")),
    battery: reading(fields.battery, "battery", battery),
    processes: reading(fields.processes, "processes", processSummary),
  };
}
