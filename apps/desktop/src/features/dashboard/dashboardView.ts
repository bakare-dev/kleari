import type { QuarantineList, ReviewList } from "../../contracts/cleanup";
import type { StorageOverview } from "../../contracts/storageAnalysis";
import type { SystemActivity, SystemDetails } from "../../contracts/systemPage";
import type { DashboardData, DashboardFinding, DashboardMetric } from "../../v2/screens/dashboard/DashboardScreen";
import { categoryMeta, eligibilityLabel } from "../../v2/screens/cleanup/findingMeta";
import { formatGb, plural } from "../../v2/ui/format";
import { findingView, when } from "../cleanup/cleanupView";
import { batteryStatusText, formatBytes, formatQuantity } from "../overview/format";
import { buildStorageView } from "../storage/storageAnalysisView";
import type { AnalysisState } from "../storage/storageAnalysisView";
import type { Loadable } from "../system/systemView";
import { liveNote } from "../system/systemView";

const GB = 1_000_000_000;
const pressureText = { normal: "pressure normal", warning: "pressure elevated", critical: "pressure high" } as const;

/** The latest reading, kept through an error; `null` before the first. */
export function value<T>(state: Loadable<T>): T | null {
  return state.status === "ready" ? state.value : state.status === "error" ? state.last : null;
}

/** The four reading cards, shared with the menu-bar popover. */
export function metrics(activity: SystemActivity | null, history: readonly SystemActivity[], details: SystemDetails | null, overview: StorageOverview | null): DashboardMetric[] {
  const volume = overview?.volume.status === "available" ? overview.volume.value : null;
  const memory = activity?.memory.status === "available" ? activity.memory.value : null;
  const pressure = activity?.memoryPressure.status === "available" ? activity.memoryPressure.value : null;
  const cpu = activity?.cpu.status === "available" ? activity.cpu.value : null;
  const cores = details?.cpuCores.status === "available" ? details.cpuCores.value.total : null;
  const battery = activity?.battery;
  const cpuHistory = history.flatMap((sample) => (sample.cpu.status === "available" ? [sample.cpu.value.usagePercent] : []));
  const percent = (used: number, total: number) => (total > 0 ? Math.round((used / total) * 100) : 0);
  return [
    volume
      ? { label: "Storage", tone: "blue", icon: "disk", value: formatQuantity(formatBytes(volume.usedBytes, "storage")), sub: `used of ${formatQuantity(formatBytes(volume.totalBytes, "storage"))}`, percent: percent(volume.usedBytes, volume.totalBytes) }
      : { label: "Storage", tone: "blue", icon: "disk", value: "—", sub: "Reading the startup disk…" },
    memory
      ? {
          label: "Memory",
          tone: "green",
          icon: "chip",
          value: formatQuantity(formatBytes(memory.usedBytes, "memory")),
          sub: `of ${formatQuantity(formatBytes(memory.totalBytes, "memory"))}${pressure ? ` · ${pressureText[pressure]}` : ""}`,
          percent: percent(memory.usedBytes, memory.totalBytes),
        }
      : { label: "Memory", tone: "green", icon: "chip", value: "—", sub: "Waiting for a reading" },
    {
      label: "CPU",
      tone: "indigo",
      icon: "pulse",
      value: cpu ? `${Math.round(cpu.usagePercent)}%` : "—",
      sub: cpu ? `average${cores ? ` · ${plural(cores, "core")}` : ""}` : "Measuring…",
      history: cpuHistory,
      historyLabel: cpuHistory.length < 2 ? "The chart fills in as readings arrive" : `CPU load over the last ${plural(cpuHistory.length, "reading")}, between ${Math.round(Math.min(...cpuHistory))} and ${Math.round(Math.max(...cpuHistory))} percent`,
    },
    battery?.status === "available"
      ? { label: "Battery", tone: "orange", icon: "battery", value: `${Math.round(battery.value.chargePercent)}%`, sub: batteryStatusText(battery.value), percent: Math.round(battery.value.chargePercent) }
      : { label: "Battery", tone: "orange", icon: "battery", value: "—", sub: battery?.status === "unavailable" && battery.reason === "notPresent" ? "No battery" : "Waiting for a reading" },
  ];
}

function findings(review: ReviewList | null, nowMs: number): DashboardFinding[] {
  return (review?.findings ?? [])
    .map((finding) => findingView(finding, nowMs))
    .filter((finding) => finding.eligibility === "eligible")
    .sort((a, b) => b.gb - a.gb)
    .slice(0, 3)
    .map((finding) => {
      const meta = categoryMeta[finding.category];
      return { title: finding.name, sub: `${meta.label} · ${eligibilityLabel[finding.eligibility].toLowerCase()} for review`, gb: finding.gb, tone: meta.tone, icon: meta.icon };
    });
}

/**
 * The Dashboard from what Kleari already has: live readings sampled since launch, the
 * startup disk's capacity, this session's last Storage analysis and Cleanup scan, and what
 * quarantine holds. Nothing here starts a scan.
 */
export function buildDashboard(input: {
  activity: Loadable<SystemActivity>;
  history: readonly SystemActivity[];
  details: Loadable<SystemDetails>;
  overview: Loadable<StorageOverview>;
  analysis: AnalysisState;
  review: ReviewList | null;
  quarantine: QuarantineList | null;
  reminderGb: number;
  nowMs: number;
}): DashboardData {
  const activity = value(input.activity);
  const storage = buildStorageView({ overview: input.overview, analysis: input.analysis, nowMs: input.nowMs });
  const result = input.analysis.result;
  const held = input.quarantine?.status === "available" ? input.quarantine.batches.flatMap((batch) => batch.items) : [];
  const heldBytes = held.reduce((total, item) => total + item.bytes, 0);
  const overReminder = input.reminderGb > 0 && heldBytes > input.reminderGb * GB;
  const scanned = input.review?.scan !== null && input.review?.scan !== undefined;
  return {
    sampledAt: activity ? new Date(activity.sampledAtMs).toLocaleTimeString("en-GB") : null,
    note: liveNote(input.activity),
    metrics: metrics(activity, input.history, value(input.details), value(input.overview)),
    storage: {
      volume: storage.volume,
      lastScan: result ? when(result.startedAtMs, input.nowMs) : null,
      usedGb: storage.capacity?.usedGb ?? 0,
      totalGb: storage.capacity?.totalGb ?? 0,
      categories: storage.categories.filter((category) => category.gb > 0).map((category) => ({ label: category.label, gb: category.gb, tone: category.tone })),
      analyzing: input.analysis.running,
    },
    quarantineCount: held.length,
    quarantineNote: overReminder ? `${formatGb(heldBytes / GB)} held, more than the ${input.reminderGb} GB you set` : undefined,
    findings: findings(input.review, input.nowMs),
    findingsEmpty: scanned ? "The last scan found nothing eligible to move." : "Run a scan to see what you could reclaim. Scans only read.",
  };
}
