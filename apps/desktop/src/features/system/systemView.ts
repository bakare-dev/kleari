// Turns validated System readings into what the System page shows. Pure: every state
// (loading, live, paused, stale, unavailable) is decided here and covered by tests.

import type { Reading, UnavailableReason } from "../../contracts/systemMetrics";
import type {
  StartupItem,
  StartupItems,
  StartupScope,
  SystemActivity,
  SystemDetails,
  ThermalState,
} from "../../contracts/systemPage";
import type { MetricsErrorKind } from "../../lib/systemMetricsIpc";
import { batteryStatusText, errorText, formatBytes, formatDuration, formatQuantity } from "../overview/format";
import type {
  ChartTab,
  ChartView,
  GaugeView,
  HealthCheck,
  LiveState,
  StartupRow,
  StartupView,
  SystemView,
} from "../../v2/screens/system/SystemScreen";
import type { Status, Tone } from "../../v2/ui/components";

/** A request's state: never loaded, loaded, or failed (keeping the last good value). */
export type Loadable<T> =
  | { status: "loading" }
  | { status: "ready"; value: T }
  | { status: "error"; error: MetricsErrorKind; last: T | null };

export const HISTORY_SLOTS = 30;

const GIB = 1024 ** 3;

function value<T>(reading: Reading<T> | undefined): T | null {
  return reading?.status === "available" ? reading.value : null;
}

function reasonOf<T>(reading: Reading<T> | undefined): UnavailableReason | null {
  return reading?.status === "unavailable" ? reading.reason : null;
}

function latest<T>(state: Loadable<T>): T | null {
  return state.status === "ready" ? state.value : state.status === "error" ? state.last : null;
}

export function liveState(activity: Loadable<SystemActivity>, paused: boolean): LiveState {
  if (activity.status === "error") {
    return activity.last ? "stale" : "unavailable";
  }
  if (paused) {
    return "paused";
  }
  // A sample whose CPU is still warming up (first visit, or just resumed) measures no short
  // window yet, so it isn't presented as live.
  return activity.status === "ready" && activity.value.cpu.status === "available" ? "live" : "waiting";
}

export function liveNote(activity: Loadable<SystemActivity>): string | null {
  if (activity.status !== "error") {
    return null;
  }
  return activity.last
    ? "The latest refresh failed. The readings below are from the last successful one."
    : errorText[activity.error];
}

/** Text for a reading that isn't available, with the request's own state taking priority. */
function missing(state: Loadable<SystemActivity>, reason: UnavailableReason | null, fallback = "Not reported"): string {
  if (state.status === "loading") {
    return "Waiting for a reading";
  }
  if (state.status === "error" && !state.last) {
    return state.error === "backendUnavailable" ? "Needs the desktop app" : "Unavailable";
  }
  if (reason === "warmingUp") {
    return "Measuring…";
  }
  if (reason === "notPresent") {
    return "No battery";
  }
  return fallback;
}

export function formatRate(bytesPerSecond: number): string {
  return `${formatQuantity(formatBytes(Math.round(bytesPerSecond), "storage"))}/s`;
}

/** macOS's thermal categories, with Apple's own plain-language meaning; not a throttling reading. */
const thermalText: Record<ThermalState, { label: string; meaning: string; status: Status; fraction: number }> = {
  nominal: { label: "Nominal", meaning: "Normal", status: "ok", fraction: 0.25 },
  fair: { label: "Fair", meaning: "Slightly elevated", status: "ok", fraction: 0.5 },
  serious: { label: "Serious", meaning: "High", status: "warn", fraction: 0.75 },
  critical: { label: "Critical", meaning: "Critical", status: "danger", fraction: 1 },
};

/** From this warm up, a battery is worth keeping an eye on; from `HOT_CELSIUS` it is hot. */
export const WARM_CELSIUS = 40;
export const HOT_CELSIUS = 45;

function temperaturePill(celsius: number, thermal: ThermalState | null): string {
  if (thermal === "serious" || thermal === "critical") {
    return `Cooling down · ${thermalText[thermal].meaning}`;
  }
  return celsius >= HOT_CELSIUS ? "Hot" : celsius >= WARM_CELSIUS ? "Warm" : "Normal";
}

function temperatureStatus(celsius: number, thermal: ThermalState | null): Status {
  if (thermal === "critical") {
    return "danger";
  }
  return thermal === "serious" || celsius >= WARM_CELSIUS ? "warn" : "ok";
}

/** A processor running warm on average, and a sensor this hot, are worth noticing; Apple
 * silicon throttles itself near 100 °C, which macOS's thermal state reports. */
export const CHIP_WARM_CELSIUS = 80;
export const CHIP_HOT_CELSIUS = 95;

function chipPill(average: number, hottest: number, thermal: ThermalState | null): string {
  if (thermal === "serious" || thermal === "critical") {
    return `Cooling down · ${thermalText[thermal].meaning}`;
  }
  return hottest >= CHIP_HOT_CELSIUS ? "Hot" : average >= CHIP_WARM_CELSIUS ? "Warm" : "Normal";
}

function chipStatus(average: number, hottest: number, thermal: ThermalState | null): Status {
  if (thermal === "critical") {
    return "danger";
  }
  return thermal === "serious" || hottest >= CHIP_HOT_CELSIUS || average >= CHIP_WARM_CELSIUS ? "warn" : "ok";
}

const pressureText = {
  normal: { label: "Pressure normal", status: "ok" },
  warning: { label: "Pressure high", status: "warn" },
  critical: { label: "Pressure critical", status: "danger" },
} as const satisfies Record<string, { label: string; status: Status }>;

export function buildGauges(state: Loadable<SystemActivity>, details: Loadable<SystemDetails>): GaugeView[] {
  const activity = latest(state);
  const info = latest(details);
  const cpu = value(activity?.cpu);
  const memory = value(activity?.memory);
  const pressure = value(activity?.memoryPressure);
  const battery = value(activity?.battery);
  const thermal = value(activity?.thermalState);
  const cores = value(info?.cpuCores);
  const health = value(info?.batteryHealth);

  const cpuGauge: GaugeView = cpu
    ? {
        label: "CPU",
        tone: "indigo",
        fraction: cpu.usagePercent / 100,
        center: `${Math.round(cpu.usagePercent)}%`,
        value: cores ? `${cores.total} cores` : "All cores",
        sub: "Average load",
        pill: cpu.usagePercent > 60 ? "Busy" : "Light load",
        status: cpu.usagePercent > 60 ? "warn" : "ok",
        aria: `CPU ${Math.round(cpu.usagePercent)} percent`,
      }
    : unavailableGauge("CPU", "indigo", missing(state, reasonOf(activity?.cpu)));

  const memoryGauge: GaugeView = memory
    ? {
        label: "Memory",
        tone: "green",
        fraction: memory.usedBytes / memory.totalBytes,
        center: `${Math.round((memory.usedBytes / memory.totalBytes) * 100)}%`,
        value: formatQuantity(formatBytes(memory.usedBytes, "memory")),
        sub: `of ${formatQuantity(formatBytes(memory.totalBytes, "memory"))}`,
        pill: pressure ? pressureText[pressure].label : "Pressure not reported",
        status: pressure ? pressureText[pressure].status : "muted",
        aria: `Memory ${Math.round((memory.usedBytes / memory.totalBytes) * 100)} percent used`,
      }
    : unavailableGauge("Memory", "green", missing(state, reasonOf(activity?.memory)));

  const healthPill =
    health?.maximumCapacityPercent !== null && health?.maximumCapacityPercent !== undefined
      ? `Health ${Math.round(health.maximumCapacityPercent)}%`
      : null;
  const batteryGauge: GaugeView = battery
    ? {
        label: "Battery",
        tone: "orange",
        fraction: battery.chargePercent / 100,
        center: `${Math.round(battery.chargePercent)}%`,
        ...batteryLines(battery.state, battery.minutesRemaining),
        pill: healthPill ?? (battery.powerSource === "ac" ? "On power" : "On battery"),
        status: "ok",
        aria: `Battery ${Math.round(battery.chargePercent)} percent, ${batteryStatusText(battery)}`,
      }
    : unavailableGauge("Battery", "orange", missing(state, reasonOf(activity?.battery)));

  // The processor's own sensors first (ADR-023), then the battery sensor, then macOS's
  // thermal state alone (whether the Mac is slowing itself down to cool). Each is named for
  // what it is.
  const chip = value(activity?.chipTemperature);
  const celsius = value(activity?.batteryTemperature);
  const stateText = thermal ? `macOS thermal state: ${thermalText[thermal].label}` : "Thermal state not reported";
  const thermalGauge: GaugeView = chip
    ? {
        label: "Temperature",
        tone: "teal",
        // 30 °C to 105 °C across the ring.
        fraction: Math.min(1, Math.max(0, (chip.cpu - 30) / 75)),
        center: `${Math.round(chip.cpu)}°`,
        value: `${chip.cpu.toFixed(1)} °C`,
        sub: [
          `Processor sensors, hottest ${Math.round(chip.cpuMax)} °C`,
          chip.gpu !== null ? `graphics ${Math.round(chip.gpu)} °C` : null,
          celsius !== null ? `battery ${Math.round(celsius)} °C` : null,
        ]
          .filter(Boolean)
          .join(" · "),
        pill: chipPill(chip.cpu, chip.cpuMax, thermal),
        status: chipStatus(chip.cpu, chip.cpuMax, thermal),
        aria: `Processor temperature ${chip.cpu.toFixed(1)} degrees Celsius, hottest sensor ${Math.round(chip.cpuMax)}. ${stateText}`,
      }
    : celsius !== null
      ? {
          label: "Temperature",
          tone: "teal",
          // 20 °C to 50 °C across the ring.
          fraction: Math.min(1, Math.max(0, (celsius - 20) / 30)),
          center: `${Math.round(celsius)}°`,
          value: `${celsius.toFixed(1)} °C`,
          sub: `Battery sensor · ${stateText}`,
          pill: temperaturePill(celsius, thermal),
          status: temperatureStatus(celsius, thermal),
          aria: `Battery temperature ${celsius.toFixed(1)} degrees Celsius. ${stateText}`,
        }
      : thermal
        ? {
            label: "Temperature",
            tone: "teal",
            fraction: thermalText[thermal].fraction,
            center: "",
            value: thermalText[thermal].label,
            sub: "macOS thermal state · this Mac has no battery sensor",
            pill: thermalText[thermal].meaning,
            status: thermalText[thermal].status,
            aria: `Thermal state ${thermalText[thermal].label}`,
          }
        : unavailableGauge("Temperature", "teal", missing(state, reasonOf(activity?.thermalState)));

  return [cpuGauge, memoryGauge, batteryGauge, thermalGauge];
}

function batteryLines(state: "charging" | "charged" | "notCharging" | "discharging", minutes: number | null): { value: string; sub: string } {
  switch (state) {
    case "charging":
      return minutes === null ? { value: "Charging", sub: "calculating time to full" } : { value: formatDuration(minutes), sub: "until full" };
    case "discharging":
      return minutes === null ? { value: "On battery", sub: "calculating time left" } : { value: formatDuration(minutes), sub: "remaining" };
    case "charged":
      return { value: "Fully charged", sub: "on power adapter" };
    case "notCharging":
      return { value: "Not charging", sub: "on power adapter" };
  }
}

function unavailableGauge(label: string, tone: Tone, why: string): GaugeView {
  return { label, tone, fraction: null, center: "—", value: why, sub: "", pill: "Unavailable", status: "muted", aria: `${label} unavailable` };
}

/** Rounds a chart ceiling up to 1, 2 or 5 times a power of ten, never below `floor`. */
export function niceCeiling(maxValue: number, floor: number): number {
  const target = Math.max(maxValue, floor);
  const magnitude = 10 ** Math.floor(Math.log10(target));
  const step = [1, 2, 5, 10].find((candidate) => candidate * magnitude >= target) ?? 10;
  return step * magnitude;
}

export function buildCharts(history: readonly SystemActivity[]): Record<ChartTab, ChartView> {
  // One slot per sample, unavailable ones kept as null, so a missing reading is drawn as a
  // gap rather than squeezing the readings either side of it together.
  const series = (pick: (sample: SystemActivity) => number | null): (number | null)[] => history.map(pick);
  const cpu = series((sample) => value(sample.cpu)?.usagePercent ?? null);
  const memory = series((sample) => {
    const reading = value(sample.memory);
    return reading ? reading.usedBytes / GIB : null;
  });
  const memoryTotal = history.map((sample) => value(sample.memory)?.totalBytes).find((total) => total !== undefined);
  const down = series((sample) => value(sample.network)?.receivedBytesPerSecond ?? null);
  const up = series((sample) => value(sample.network)?.transmittedBytesPerSecond ?? null);
  const read = series((sample) => value(sample.disk)?.readBytesPerSecond ?? null);
  const write = series((sample) => value(sample.disk)?.writtenBytesPerSecond ?? null);
  // "Now" is the latest sample only: if it has no reading, nothing older stands in for it.
  const now = (values: readonly (number | null)[], format: (entry: number) => string) => {
    const latestValue = values[values.length - 1];
    return latestValue === undefined || latestValue === null ? "—" : format(latestValue);
  };
  const known = (...lists: (number | null)[][]) => lists.flat().filter((entry): entry is number => entry !== null);
  const memoryMax = memoryTotal ? memoryTotal / GIB : niceCeiling(Math.max(0, ...known(memory)), 1);
  const networkMax = niceCeiling(Math.max(0, ...known(down, up)), 100_000);
  const diskMax = niceCeiling(Math.max(0, ...known(read, write)), 10_000_000);
  return {
    cpu: { max: 100, scale: "0 to 100%", note: "Average across all cores", series: [{ name: "Usage", values: cpu, now: now(cpu, (entry) => `${Math.round(entry)}%`), tone: "indigo" }] },
    memory: {
      max: memoryMax,
      scale: `0 to ${formatQuantity(formatBytes(memoryMax * GIB, "memory"))}`,
      note: "Memory in use",
      series: [{ name: "Used", values: memory, now: now(memory, (entry) => formatQuantity(formatBytes(entry * GIB, "memory"))), tone: "green" }],
    },
    network: {
      max: networkMax,
      scale: `0 to ${formatRate(networkMax)}`,
      note: "All network interfaces, tunnels excluded",
      series: [
        { name: "Down", values: down, now: now(down, formatRate), tone: "blue" },
        { name: "Up", values: up, now: now(up, formatRate), tone: "teal" },
      ],
    },
    disk: {
      max: diskMax,
      scale: `0 to ${formatRate(diskMax)}`,
      note: "Startup disk",
      series: [
        { name: "Read", values: read, now: now(read, formatRate), tone: "orange" },
        { name: "Write", values: write, now: now(write, formatRate), tone: "pink" },
      ],
    },
  };
}

export function formatUptime(bootTimeMs: number, nowMs: number): string {
  const minutes = Math.max(0, Math.floor((nowMs - bootTimeMs) / 60_000));
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  if (days > 0) {
    return `${days} ${days === 1 ? "day" : "days"} ${hours} h`;
  }
  return hours > 0 ? `${hours} h ${minutes % 60} min` : `${minutes % 60} min`;
}

function readingText<T>(reading: Reading<T>, format: (inner: T) => string): string {
  if (reading.status === "available") {
    return format(reading.value);
  }
  return reading.reason === "notPresent" ? "None" : "Not reported";
}

export function buildMac(details: Loadable<SystemDetails>, nowMs: number): Pick<SystemView, "mac" | "specs"> {
  const info = latest(details);
  if (!info) {
    const message = details.status === "loading" ? "Loading…" : details.status === "error" ? errorText[details.error] : "Not reported";
    return { mac: { title: "This Mac", detail: message }, specs: [] };
  }
  const model = value(info.model);
  const os = value(info.os);
  const cores = value(info.cpuCores);
  const gpu = value(info.gpuCores);
  const coreText = cores
    ? `${cores.total} CPU${cores.performance !== null && cores.efficiency !== null ? ` (${cores.performance}P + ${cores.efficiency}E)` : ""}${gpu !== null ? ` · ${gpu} GPU` : ""}`
    : "Not reported";
  return {
    mac: {
      title: model ? (model.name ?? model.identifier) : "This Mac",
      detail: [model?.name ? model.identifier : null, os ? `macOS ${os.name ? `${os.name} ` : ""}${os.version}` : null].filter(Boolean).join(" · ") || "Not reported",
    },
    specs: [
      { label: "Chip", value: readingText(info.chip, (chip) => chip) },
      { label: "Cores", value: coreText },
      { label: "Memory", value: readingText(info.memoryBytes, (bytes) => formatQuantity(formatBytes(bytes, "memory"))) },
      { label: "Startup disk", value: readingText(info.startupVolume, (volume) => `${volume.name} · ${formatQuantity(formatBytes(volume.totalBytes, "storage"))}`) },
      { label: "Display", value: readingText(info.display, (display) => `${display.diagonalInches !== null ? `${display.diagonalInches}″ · ` : ""}${display.name}`) },
      { label: "Uptime", value: readingText(info.bootTimeMs, (boot) => formatUptime(boot, nowMs)) },
    ],
  };
}

export function buildHealth(details: Loadable<SystemDetails>, activity: Loadable<SystemActivity> = { status: "loading" }): { health: HealthCheck[]; summary: string } {
  const info = latest(details);
  const checks: HealthCheck[] = [];
  const health = info ? info.batteryHealth : null;
  if (health?.status === "available") {
    const { maximumCapacityPercent: capacity, cycleCount: cycles, designCycleCount: designed, serviceRecommended } = health.value;
    // macOS's own verdict decides, as in System Settings › Battery; capacity alone doesn't.
    checks.push({
      label: "Battery",
      sub: [
        cycles !== null ? `${cycles}${designed !== null ? ` of ${designed}` : ""} cycles` : null,
        capacity !== null ? `Maximum capacity ${Math.round(capacity)}%` : null,
      ]
        .filter(Boolean)
        .join(" · "),
      status: serviceRecommended ? "Service recommended" : "Normal",
      tone: serviceRecommended ? "warn" : "ok",
      tile: "orange",
      icon: "battery",
    });
  } else if (health?.status === "unavailable" && health.reason !== "notPresent") {
    checks.push({ label: "Battery", sub: "macOS did not report battery health", tone: "muted", status: "Not reported", tile: "orange", icon: "battery" });
  }
  // Cooling: the fans and the processor's sensors (ADR-023), read-only.
  const live = latest(activity);
  const fans = value(live?.fans);
  const chip = value(live?.chipTemperature);
  if (fans !== null || chip !== null) {
    const rpm = (value: number) => `${Math.round(value).toLocaleString("en-US")} rpm`;
    const fanText =
      fans === null
        ? null
        : fans.length === 0
          ? "No fans"
          : fans
              .map((fan, index) => `${fans.length > 1 ? `Fan ${index + 1} ` : "Fan "}${rpm(fan.rpm)}${fan.minRpm !== null && fan.maxRpm !== null ? ` (${rpm(fan.minRpm).replace(" rpm", "")}–${rpm(fan.maxRpm)})` : ""}`)
              .join(" · ");
    const hot = chip !== null && (chip.cpuMax >= CHIP_HOT_CELSIUS || chip.cpu >= CHIP_WARM_CELSIUS);
    checks.push({
      label: "Cooling",
      sub: [fanText, chip ? `processor ${Math.round(chip.cpu)} °C, hottest ${Math.round(chip.cpuMax)} °C` : null].filter(Boolean).join(" · "),
      status: hot ? "Running warm" : "Normal",
      tone: hot ? "warn" : "ok",
      tile: "teal",
      icon: "fan",
    });
  }
  const os = info ? value(info.os) : null;
  checks.push({
    label: "macOS",
    sub: os ? `${os.name ? `${os.name} ` : ""}${os.version} installed. Kleari doesn’t check for macOS updates; Software Update does.` : "Check for updates in Software Update.",
    tone: "muted",
    tile: "indigo",
    icon: "refresh",
    action: { label: "Open Software Update", pane: "softwareUpdate" },
  });
  return { health: checks, summary: "Read from this Mac" };
}

const scopeText: Record<StartupScope, string> = {
  userAgent: "Agent · runs when you log in",
  allUsersAgent: "Agent · runs for every user",
  systemDaemon: "Daemon · runs at startup",
};

const tileTones: readonly Tone[] = ["blue", "teal", "indigo", "orange", "green", "pink", "purple", "red"];

/** A stable tile colour per name, so an item keeps its colour between visits. */
export function toneFor(name: string): Tone {
  let hash = 0;
  for (const character of name) {
    hash = (hash * 31 + (character.codePointAt(0) ?? 0)) >>> 0;
  }
  return tileTones[hash % tileTones.length];
}

/**
 * Statuses say only what Kleari checked: whether the program file is there, and the plist's
 * own Disabled key. Whether macOS lets the item run is in System Settings, not shown here.
 */
function startupRow(item: StartupItem): StartupRow {
  const program = {
    present: { status: "Program found", tone: "muted" as const },
    missing: { status: "Program missing", tone: "warn" as const },
    notChecked: { status: "Not checked", tone: "muted" as const },
  }[item.program];
  const status = item.program !== "missing" && item.disabledInPlist ? { status: "Plist: disabled", tone: "muted" as const } : program;
  return { name: item.name, kind: scopeText[item.scope], tile: toneFor(item.name), ...status };
}

export function buildStartup(state: Loadable<StartupItems>): StartupView {
  const list = latest(state);
  if (!list) {
    return {
      state: state.status === "loading" ? "loading" : "unavailable",
      message: state.status === "loading" ? "Reading launch agents and daemons…" : errorText[state.status === "error" ? state.error : "unknown"],
      rows: [],
      summary: "",
      counts: "",
    };
  }
  const rows = list.items.map(startupRow);
  const missingCount = list.items.filter((item) => item.program === "missing").length;
  const agents = list.items.filter((item) => item.scope !== "systemDaemon").length;
  const notes = [
    missingCount
      ? `${missingCount} ${missingCount === 1 ? "starts a program that is" : "start programs that are"} missing, usually left behind by a removed app.`
      : null,
    list.unreadableCount ? `${list.unreadableCount} couldn't be read.` : null,
    list.unreadableFolderCount
      ? `${list.unreadableFolderCount} ${list.unreadableFolderCount === 1 ? "folder" : "folders"} couldn't be listed, so this list may be incomplete.`
      : null,
    list.truncated ? "Some folders had too many items to list." : null,
  ].filter(Boolean);
  return {
    state: "ready",
    rows,
    summary: `${rows.length} background ${rows.length === 1 ? "item" : "items"} found.${notes.length ? ` ${notes.join(" ")}` : ""}`,
    counts: `${rows.length} items · ${agents} agents · ${rows.length - agents} daemons`,
  };
}

/**
 * How long ago the oldest charted reading was taken, rounded for the chart's title and axis:
 * seconds under two minutes, whole minutes after. `null` with fewer than two readings.
 * Readings are taken every second while System is open and less often otherwise, so the
 * chart is labelled by the time it actually covers, not by its number of slots.
 */
export function historySpan(history: readonly SystemActivity[]): string | null {
  const first = history[0];
  const last = history[history.length - 1];
  if (first === undefined || last === undefined || history.length < 2) {
    return null;
  }
  const seconds = Math.max(1, Math.round((last.sampledAtMs - first.sampledAtMs) / 1000));
  return seconds < 120 ? `${seconds} s` : `${Math.round(seconds / 60)} min`;
}

/** Keeps the last `HISTORY_SLOTS` samples; a gap (pause, hidden window) starts a new chart. */
export function appendHistory(history: readonly SystemActivity[], sample: SystemActivity, maxGapMs: number): SystemActivity[] {
  const previous = history[history.length - 1];
  const continuous = previous !== undefined && sample.sampledAtMs - previous.sampledAtMs <= maxGapMs;
  const next = continuous ? [...history, sample] : [sample];
  return next.slice(-HISTORY_SLOTS);
}

export function buildSystemView(input: {
  activity: Loadable<SystemActivity>;
  history: readonly SystemActivity[];
  details: Loadable<SystemDetails>;
  startup: Loadable<StartupItems>;
  paused: boolean;
  nowMs: number;
}): SystemView {
  const health = buildHealth(input.details, input.activity);
  return {
    live: liveState(input.activity, input.paused),
    note: liveNote(input.activity),
    gauges: buildGauges(input.activity, input.details),
    charts: buildCharts(input.history),
    slots: HISTORY_SLOTS,
    span: historySpan(input.history),
    ...buildMac(input.details, input.nowMs),
    health: health.health,
    healthSummary: health.summary,
    startup: buildStartup(input.startup),
  };
}
