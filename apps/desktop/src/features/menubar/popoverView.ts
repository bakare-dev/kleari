import type { QuarantineList, ReviewList } from "../../contracts/cleanup";
import type { Issue } from "../../contracts/issues";
import type { StorageOverview } from "../../contracts/storageAnalysis";
import type { SystemActivity, SystemDetails } from "../../contracts/systemPage";
import type { PopoverAlert, PopoverGauge, PopoverLink, PopoverNetwork, PopoverSummary } from "../../v2/screens/popover/MenuBarPopover";
import { formatGb, plural, sum } from "../../v2/ui/format";
import { findingView, when } from "../cleanup/cleanupView";
import { metrics, value } from "../dashboard/dashboardView";
import { issueLook, issueText, sortedIssues } from "../issues/issueView";
import { formatRate, type Loadable } from "../system/systemView";

const GB = 1_000_000_000;

export interface PopoverData {
  gauges: PopoverGauge[];
  network: PopoverNetwork;
  alerts: PopoverAlert[];
  summary: PopoverSummary;
  links: PopoverLink[];
}

function summary(state: Loadable<ReviewList>, nowMs: number): PopoverSummary {
  const review = value(state);
  if (review === null) {
    const sub = state.status === "error" ? "Couldn’t read the last scan" : "Reading the last scan…";
    return { title: "Cleanup", sub, tone: "blue", icon: "search", review: null };
  }
  if (review.scan === null) {
    return { title: "No scan yet", sub: "Scans only read. Nothing moves until you approve it.", tone: "blue", icon: "search", review: null };
  }
  const eligible = review.findings.map((finding) => findingView(finding, nowMs)).filter((finding) => finding.eligibility === "eligible");
  const scanned = `Last scan ${when(review.scan.startedAtMs, nowMs)}${review.scan.cancelled ? " · stopped early" : ""}`;
  return eligible.length === 0
    ? { title: "Nothing to reclaim", sub: scanned, tone: "green", icon: "check", review: null }
    : { title: `${formatGb(sum(eligible, (finding) => finding.gb))} ready to review`, sub: scanned, tone: "blue", icon: "search", review: `Review ${plural(eligible.length, "item")}` };
}

const pending = (state: Loadable<unknown>) => (state.status === "error" ? "Couldn’t read" : "Reading…");

/** Says so when `item` shows an earlier reading because the latest one failed. */
function marked<T extends { sub: string }>(state: Loadable<unknown>, item: T): T {
  return state.status === "error" && state.last !== null ? { ...item, sub: `${item.sub} · not updated` } : item;
}

function quarantineLink(state: Loadable<QuarantineList>): PopoverLink {
  const base = { label: "Quarantine", route: "quarantine", tone: "orange", icon: "quarantine" } as const;
  const list = value(state);
  if (list === null) {
    return { ...base, sub: pending(state), value: "" };
  }
  if (list.status === "unavailable") {
    return { ...base, sub: "History unavailable", value: "" };
  }
  const items = list.batches.flatMap((batch) => batch.items);
  return items.length === 0
    ? { ...base, sub: "Empty", value: "" }
    : { ...base, sub: `${plural(items.length, "item")} held`, value: formatGb(sum(items, (item) => item.bytes) / GB) };
}

/** Download and upload now, from the live readings (sampled every second while open). */
function networkRates(state: Loadable<SystemActivity>): PopoverNetwork {
  const reading = value(state)?.network;
  if (reading?.status !== "available") {
    return { down: "—", up: "—", note: reading?.status === "unavailable" && reading.reason !== "warmingUp" ? "Not reported" : "Measuring…" };
  }
  return { down: formatRate(reading.value.receivedBytesPerSecond), up: formatRate(reading.value.transmittedBytesPerSecond), note: null };
}

function alerts(state: Loadable<Issue[]>): PopoverAlert[] {
  return sortedIssues(value(state) ?? []).map((issue) => {
    const look = issueLook[issue.kind];
    const { title, sub } = issueText(issue);
    return { key: `${issue.kind}-${issue.subject ?? ""}`, title, sub, warning: issue.severity === "warning", route: look.route, tone: look.tone, icon: look.icon };
  });
}

/**
 * The popover from what Kleari reads when it opens: the same live readings as the
 * Dashboard with network rates, what needs attention, the last Cleanup scan and what
 * quarantine holds.
 */
export function buildPopover(input: {
  activity: Loadable<SystemActivity>;
  details: Loadable<SystemDetails>;
  overview: Loadable<StorageOverview>;
  review: Loadable<ReviewList>;
  quarantine: Loadable<QuarantineList>;
  issues: Loadable<Issue[]>;
  nowMs: number;
}): PopoverData {
  const activity = value(input.activity);
  const cpu = activity?.cpu.status === "available" ? Math.round(activity.cpu.value.usagePercent) : null;
  // The Dashboard draws CPU as a chart; the popover has room for a meter only.
  const gauges = metrics(activity, [], value(input.details), value(input.overview)).map((metric): PopoverGauge => {
    const storage = metric.label === "Storage";
    return marked<PopoverGauge>(storage ? input.overview : input.activity, {
      label: metric.label,
      value: metric.value,
      sub: metric.sub,
      percent: metric.label === "CPU" ? cpu : (metric.percent ?? null),
      route: storage ? "storage" : "system",
      tone: metric.tone,
      icon: metric.icon,
    });
  });
  return {
    gauges,
    network: networkRates(input.activity),
    alerts: alerts(input.issues),
    summary: marked(input.review, summary(input.review, input.nowMs)),
    links: [marked(input.quarantine, quarantineLink(input.quarantine))],
  };
}
