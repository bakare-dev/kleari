// IPC contracts for the System page: live activity, hardware details and startup items.
// Mirrors src-tauri/src/system_monitor/{activity,details,startup}.rs; every response is
// validated before the UI sees it.

import {
  ContractError,
  battery,
  capacity,
  nonNegativeInteger,
  nullable,
  oneOf,
  percent,
  readingOf,
  reading,
  record,
  type BatteryReading,
  type CpuReading,
  type MemoryReading,
  type Reading,
} from "./systemMetrics";

export type MemoryPressure = "normal" | "warning" | "critical";
export type ThermalState = "nominal" | "fair" | "serious" | "critical";

export interface SwapReading {
  totalBytes: number;
  usedBytes: number;
}

export interface NetworkRates {
  receivedBytesPerSecond: number;
  transmittedBytesPerSecond: number;
  windowMs: number;
}

export interface DiskRates {
  readBytesPerSecond: number;
  writtenBytesPerSecond: number;
  windowMs: number;
}

/** Processor (and graphics, where the chip names them) temperatures in °C. */
export interface ChipTemperatures {
  /** The average of the processor's sensors. */
  cpu: number;
  /** The hottest processor sensor. */
  cpuMax: number;
  gpu: number | null;
}

export interface Fan {
  rpm: number;
  minRpm: number | null;
  maxRpm: number | null;
}

export interface SystemActivity {
  sampledAtMs: number;
  cpu: Reading<CpuReading>;
  memory: Reading<MemoryReading>;
  swap: Reading<SwapReading>;
  memoryPressure: Reading<MemoryPressure>;
  thermalState: Reading<ThermalState>;
  /** The battery sensor's temperature in °C (the one macOS publishes); not the chip's. */
  batteryTemperature: Reading<number>;
  /** The processor's temperature from the chip's own sensors (°C). */
  chipTemperature: Reading<ChipTemperatures>;
  /** Fan speeds; empty on a Mac without fans. */
  fans: Reading<Fan[]>;
  battery: Reading<BatteryReading>;
  network: Reading<NetworkRates>;
  disk: Reading<DiskRates>;
}

export interface ModelInfo {
  identifier: string;
  name: string | null;
}

export interface CoreCounts {
  total: number;
  performance: number | null;
  efficiency: number | null;
}

export interface OsInfo {
  version: string;
  name: string | null;
}

export interface DisplayInfo {
  name: string;
  builtIn: boolean;
  diagonalInches: number | null;
}

export interface VolumeInfo {
  name: string;
  totalBytes: number;
}

export interface BatteryHealth {
  cycleCount: number | null;
  /** The cycles the battery is designed for, when reported. */
  designCycleCount: number | null;
  /** macOS's own "Maximum Capacity", as System Information shows it. */
  maximumCapacityPercent: number | null;
  /** macOS says the battery should be serviced. */
  serviceRecommended: boolean;
}

export interface SystemDetails {
  model: Reading<ModelInfo>;
  chip: Reading<string>;
  cpuCores: Reading<CoreCounts>;
  gpuCores: Reading<number>;
  memoryBytes: Reading<number>;
  os: Reading<OsInfo>;
  display: Reading<DisplayInfo>;
  startupVolume: Reading<VolumeInfo>;
  bootTimeMs: Reading<number>;
  /** When the Mac last woke from sleep; unavailable when it hasn't slept since it started. */
  lastWakeMs: Reading<number>;
  batteryHealth: Reading<BatteryHealth>;
}

export type StartupScope = "userAgent" | "allUsersAgent" | "systemDaemon";
/** `notChecked` covers relative paths, symlinks, network volumes and errors. */
export type ProgramStatus = "present" | "missing" | "notChecked";

export interface StartupItem {
  name: string;
  label: string;
  scope: StartupScope;
  program: ProgramStatus;
  /** The plist's own Disabled key; System Settings overrides aren't visible to Kleari. */
  disabledInPlist: boolean;
}

export interface StartupItems {
  items: StartupItem[];
  unreadableCount: number;
  unreadableFolderCount: number;
  truncated: boolean;
}

/** Values are shown on screen; bounding them keeps a malformed reply from flooding the UI. */
const MAX_TEXT = 200;
const MAX_STARTUP_ITEMS = 1500;

function text(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_TEXT) {
    throw new ContractError(field);
  }
  return value;
}

/** A plausible chip temperature in °C. */
function chipCelsius(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 5 || value > 130) {
    throw new ContractError(field);
  }
  return value;
}

/** A plausible fan speed. */
function rpm(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0 || value > 20_000) {
    throw new ContractError(field);
  }
  return value;
}

/** A plausible sensor temperature in °C. */
function celsius(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < -20 || value > 90) {
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

function boolean(value: unknown, field: string): boolean {
  if (typeof value !== "boolean") {
    throw new ContractError(field);
  }
  return value;
}

const pressures: readonly MemoryPressure[] = ["normal", "warning", "critical"];
const thermalStates: readonly ThermalState[] = ["nominal", "fair", "serious", "critical"];
const scopes: readonly StartupScope[] = ["userAgent", "allUsersAgent", "systemDaemon"];
const programStatuses: readonly ProgramStatus[] = ["present", "missing", "notChecked"];

export function parseSystemActivity(payload: unknown): SystemActivity {
  const fields = record(payload, "$");
  return {
    sampledAtMs: nonNegativeInteger(fields.sampledAtMs, "sampledAtMs"),
    cpu: reading(fields.cpu, "cpu", (cpu) => ({
      usagePercent: percent(cpu.usagePercent, "cpu.usagePercent"),
      windowMs: nonNegativeInteger(cpu.windowMs, "cpu.windowMs"),
    })),
    memory: reading(fields.memory, "memory", (memory) => capacity(memory, "memory")),
    swap: reading(fields.swap, "swap", (swap) => {
      const totalBytes = nonNegativeInteger(swap.totalBytes, "swap.totalBytes");
      const usedBytes = nonNegativeInteger(swap.usedBytes, "swap.usedBytes");
      if (usedBytes > totalBytes) {
        throw new ContractError("swap");
      }
      return { totalBytes, usedBytes };
    }),
    memoryPressure: readingOf(fields.memoryPressure, "memoryPressure", (value) => oneOf(pressures, value, "memoryPressure.value")),
    thermalState: readingOf(fields.thermalState, "thermalState", (value) => oneOf(thermalStates, value, "thermalState.value")),
    batteryTemperature: readingOf(fields.batteryTemperature, "batteryTemperature", (value) => celsius(value, "batteryTemperature.value")),
    chipTemperature: reading(fields.chipTemperature, "chipTemperature", (chip) => ({
      cpu: chipCelsius(chip.cpu, "chipTemperature.cpu"),
      cpuMax: chipCelsius(chip.cpuMax, "chipTemperature.cpuMax"),
      gpu: nullable(chip.gpu, (value) => chipCelsius(value, "chipTemperature.gpu")),
    })),
    fans: readingOf(fields.fans, "fans", (value) => {
      if (!Array.isArray(value) || value.length > 8) {
        throw new ContractError("fans.value");
      }
      return value.map((entry, index) => {
        const fan = record(entry, `fans[${index}]`);
        return {
          rpm: rpm(fan.rpm, `fans[${index}].rpm`),
          minRpm: nullable(fan.minRpm, (inner) => rpm(inner, `fans[${index}].minRpm`)),
          maxRpm: nullable(fan.maxRpm, (inner) => rpm(inner, `fans[${index}].maxRpm`)),
        };
      });
    }),
    battery: reading(fields.battery, "battery", battery),
    network: reading(fields.network, "network", (network) => ({
      receivedBytesPerSecond: finiteNonNegative(network.receivedBytesPerSecond, "network.receivedBytesPerSecond"),
      transmittedBytesPerSecond: finiteNonNegative(network.transmittedBytesPerSecond, "network.transmittedBytesPerSecond"),
      windowMs: nonNegativeInteger(network.windowMs, "network.windowMs"),
    })),
    disk: reading(fields.disk, "disk", (disk) => ({
      readBytesPerSecond: finiteNonNegative(disk.readBytesPerSecond, "disk.readBytesPerSecond"),
      writtenBytesPerSecond: finiteNonNegative(disk.writtenBytesPerSecond, "disk.writtenBytesPerSecond"),
      windowMs: nonNegativeInteger(disk.windowMs, "disk.windowMs"),
    })),
  };
}

export function parseSystemDetails(payload: unknown): SystemDetails {
  const fields = record(payload, "$");
  const optionalText = (value: unknown, field: string) => nullable(value, (inner) => text(inner, field));
  const optionalCount = (value: unknown, field: string) => nullable(value, (inner) => nonNegativeInteger(inner, field));
  return {
    model: reading(fields.model, "model", (model) => ({
      identifier: text(model.identifier, "model.identifier"),
      name: optionalText(model.name, "model.name"),
    })),
    chip: readingOf(fields.chip, "chip", (value) => text(value, "chip.value")),
    cpuCores: reading(fields.cpuCores, "cpuCores", (cores) => ({
      total: nonNegativeInteger(cores.total, "cpuCores.total"),
      performance: optionalCount(cores.performance, "cpuCores.performance"),
      efficiency: optionalCount(cores.efficiency, "cpuCores.efficiency"),
    })),
    gpuCores: readingOf(fields.gpuCores, "gpuCores", (value) => nonNegativeInteger(value, "gpuCores.value")),
    memoryBytes: readingOf(fields.memoryBytes, "memoryBytes", (value) => nonNegativeInteger(value, "memoryBytes.value")),
    os: reading(fields.os, "os", (os) => ({
      version: text(os.version, "os.version"),
      name: optionalText(os.name, "os.name"),
    })),
    display: reading(fields.display, "display", (display) => ({
      name: text(display.name, "display.name"),
      builtIn: boolean(display.builtIn, "display.builtIn"),
      diagonalInches: nullable(display.diagonalInches, (value) => finiteNonNegative(value, "display.diagonalInches")),
    })),
    startupVolume: reading(fields.startupVolume, "startupVolume", (volume) => ({
      name: text(volume.name, "startupVolume.name"),
      totalBytes: nonNegativeInteger(volume.totalBytes, "startupVolume.totalBytes"),
    })),
    bootTimeMs: readingOf(fields.bootTimeMs, "bootTimeMs", (value) => nonNegativeInteger(value, "bootTimeMs.value")),
    lastWakeMs: readingOf(fields.lastWakeMs, "lastWakeMs", (value) => nonNegativeInteger(value, "lastWakeMs.value")),
    batteryHealth: reading(fields.batteryHealth, "batteryHealth", (health) => ({
      cycleCount: optionalCount(health.cycleCount, "batteryHealth.cycleCount"),
      designCycleCount: optionalCount(health.designCycleCount, "batteryHealth.designCycleCount"),
      maximumCapacityPercent: nullable(health.maximumCapacityPercent, (value) => percent(value, "batteryHealth.maximumCapacityPercent")),
      serviceRecommended: boolean(health.serviceRecommended, "batteryHealth.serviceRecommended"),
    })),
  };
}

export function parseStartupItems(payload: unknown): StartupItems {
  const fields = record(payload, "$");
  if (!Array.isArray(fields.items) || fields.items.length > MAX_STARTUP_ITEMS) {
    throw new ContractError("items");
  }
  return {
    items: fields.items.map((entry: unknown, index) => {
      const item = record(entry, `items[${index}]`);
      return {
        name: text(item.name, `items[${index}].name`),
        label: text(item.label, `items[${index}].label`),
        scope: oneOf(scopes, item.scope, `items[${index}].scope`),
        program: oneOf(programStatuses, item.program, `items[${index}].program`),
        disabledInPlist: boolean(item.disabledInPlist, `items[${index}].disabledInPlist`),
      };
    }),
    unreadableCount: nonNegativeInteger(fields.unreadableCount, "unreadableCount"),
    unreadableFolderCount: nonNegativeInteger(fields.unreadableFolderCount, "unreadableFolderCount"),
    truncated: boolean(fields.truncated, "truncated"),
  };
}
