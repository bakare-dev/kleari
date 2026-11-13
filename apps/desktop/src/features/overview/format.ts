import type { BatteryReading, UnavailableReason } from "../../contracts/systemMetrics";
import type { MetricsErrorKind } from "../../lib/systemMetricsIpc";

export interface Quantity {
  figure: string;
  unit: string;
}

const UNITS = ["bytes", "KB", "MB", "GB", "TB", "PB"];

/**
 * Formats a byte count the way macOS does: storage in decimal units (1 GB = 10⁹ bytes),
 * memory in binary units (1 GB = 1024³ bytes).
 */
export function formatBytes(bytes: number, kind: "storage" | "memory"): Quantity {
  const base = kind === "storage" ? 1000 : 1024;
  let value = bytes;
  let unitIndex = 0;
  while (value >= base && unitIndex < UNITS.length - 1) {
    value /= base;
    unitIndex += 1;
  }
  const digits = unitIndex === 0 || value >= 100 ? 0 : 1;
  return {
    figure: value.toLocaleString("en-US", { maximumFractionDigits: digits }),
    unit: UNITS[unitIndex],
  };
}

export function formatQuantity({ figure, unit }: Quantity): string {
  return `${figure} ${unit}`;
}

export function formatPercent(percent: number): string {
  return `${Math.round(percent)}%`;
}

export function formatSeconds(ms: number): string {
  return `${Math.max(1, Math.round(ms / 1000))} s`;
}

export function formatClockTime(epochMs: number): string {
  return new Date(epochMs).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function formatDuration(minutes: number): string {
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) {
    return `${rest} min`;
  }
  return rest === 0 ? `${hours} h` : `${hours} h ${rest} min`;
}

/** One status line for the battery card; a missing estimate reads as "calculating". */
export function batteryStatusText({ state, minutesRemaining }: BatteryReading): string {
  switch (state) {
    case "charging":
      return minutesRemaining === null
        ? "Charging · calculating time to full"
        : `Charging · ${formatDuration(minutesRemaining)} until full`;
    case "discharging":
      return minutesRemaining === null
        ? "On battery · calculating time remaining"
        : `On battery · ${formatDuration(minutesRemaining)} remaining`;
    case "charged":
      return "Fully charged · on power adapter";
    case "notCharging":
      return "Not charging · on power adapter";
  }
}

export const unavailableReasonText: Record<UnavailableReason, string> = {
  warmingUp: "Measuring. A reading appears after the next refresh.",
  notReported: "macOS did not report this reading.",
  notFound: "The startup volume could not be identified.",
  notPresent: "This Mac has no battery.",
};

export const errorText: Record<MetricsErrorKind, string> = {
  backendUnavailable:
    "System readings need the Kleari desktop app. This browser preview cannot read your Mac.",
  metricsUnavailable: "Kleari could not read system metrics.",
  invalidResponse: "Kleari received an unexpected response and did not display it.",
  unknown: "Something went wrong while reading system metrics.",
};
