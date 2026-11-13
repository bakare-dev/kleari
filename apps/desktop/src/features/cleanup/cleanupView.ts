// Maps validated Cleanup, Review and Quarantine data to the v2 screens' props. Every decision
// (what is eligible, why not) was made in Rust; this only words it.

import type {
  CleanupScanResult,
  EvidenceEntry,
  FolderAccess,
  HistoryUnavailableReason,
  MoveFailure,
  QuarantineBatchView,
  QuarantineItemView,
  ReviewFinding,
  ReviewList,
  ScanMode,
  ScanOptionsInput,
  ScopeFolder as ScopeFolderData,
  StepId,
  WhyNot,
} from "../../contracts/cleanup";
import type { FileCategory } from "../../contracts/storage";
import { categoryMeta } from "../../v2/screens/cleanup/findingMeta";
import type {
  Finding,
  FindingCategory,
  LastScanSummary,
  ScanModeDefinition,
  ScanOptions,
  ScanResultLine,
  ScopeFolder,
} from "../../v2/screens/cleanup/types";
import type { QuarantineBatch, QuarantineItem } from "../../v2/screens/quarantine/types";
import { formatGb, plural } from "../../v2/ui/format";
import { ago, cacheMeta } from "../developer/developerView";

const GB = 1_000_000_000;
const DAY_MS = 86_400_000;

export const toGb = (bytes: number) => bytes / GB;

/** The menu's options as the backend takes them. */
export function scanOptionsInput(options: ScanOptions): ScanOptionsInput {
  const logAge = { "7 days": 7, "30 days": 30, "90 days": 90 } as const;
  const size = { "500 MB": 500, "1 GB": 1000, "5 GB": 5000 } as const;
  const age = { "3 months": 90, "6 months": 180, "1 year": 365 } as const;
  return {
    logs: options.logs,
    temp: options.temp,
    logAgeDays: logAge[options.logAge],
    largeMinMb: size[options.size],
    oldDays: age[options.age],
    hidden: options.hidden,
    xcode: options.xcode,
    sim: options.sim,
    node: options.node,
    docker: options.docker,
  };
}

/** The menu's options from the backend's values (the defaults saved in Settings). */
export function scanOptionsFromInput(input: ScanOptionsInput): ScanOptions {
  const logAge = { 7: "7 days", 30: "30 days", 90: "90 days" } as const;
  const size = { 500: "500 MB", 1000: "1 GB", 5000: "5 GB" } as const;
  const age = { 90: "3 months", 180: "6 months", 365: "1 year" } as const;
  return {
    logs: input.logs,
    temp: input.temp,
    logAge: logAge[input.logAgeDays],
    size: size[input.largeMinMb],
    age: age[input.oldDays],
    hidden: input.hidden,
    xcode: input.xcode,
    sim: input.sim,
    node: input.node,
    docker: input.docker,
  };
}

/** The steps a mode runs with these options, in the backend's order (`scan::steps`). */
export function stepsFor(mode: ScanMode, options: ScanOptions): StepId[] {
  if (mode === "dev") {
    return [
      ...(options.xcode ? (["xcode"] as const) : []),
      ...(options.sim ? (["simulators"] as const) : []),
      ...(options.node ? (["nodeModules"] as const) : []),
      ...(options.docker ? (["docker"] as const) : []),
    ];
  }
  return [
    "appCaches",
    ...(options.logs || options.temp ? (["logsAndTemp"] as const) : []),
    "packageCaches",
    ...(mode === "deep" ? (["largeAndOld"] as const) : []),
  ];
}

const stepLabels: Record<StepId, string> = {
  appCaches: "Reading ~/Library/Caches",
  logsAndTemp: "Checking logs and temporary files",
  packageCaches: "Measuring package caches",
  largeAndOld: "Finding large and old files",
  xcode: "Reading Xcode data",
  simulators: "Reading the iOS Simulator",
  nodeModules: "Checking node_modules lockfiles",
  docker: "Reading Docker’s disk use",
};

interface ModeText {
  title: string;
  name: string;
  sub: string;
  meta: string;
  tone: ScanModeDefinition["tone"];
  icon: ScanModeDefinition["icon"];
  note: string;
  foot: string;
}

const modeText: Record<ScanMode, ModeText> = {
  standard: {
    title: "Standard scan",
    name: "standard scan",
    sub: "App caches, logs and temporary files",
    meta: "Recommended · usually under a minute",
    tone: "blue",
    icon: "search",
    note: "",
    foot: "Read-only · usually under a minute",
  },
  deep: {
    title: "Deep scan",
    name: "deep scan",
    sub: "Adds large and old files for you to inspect",
    meta: "Takes longer · large files are inspect only",
    tone: "violet",
    icon: "star",
    note: "Large and old files are never eligible for quarantine. Kleari lists them so you can decide in Finder.",
    foot: "Read-only · a few minutes",
  },
  dev: {
    title: "Developer scan",
    name: "developer scan",
    sub: "Xcode, simulators, node_modules, Docker (read-only)",
    meta: "Each tool explained separately",
    tone: "rose",
    icon: "code",
    note: "Docker stays read-only. Kleari never stops, removes or prunes containers, images or volumes.",
    foot: "Read-only · 1 to 2 minutes",
  },
};

const folderNotes: Record<ScopeFolderData["id"], { note: string; tone: ScopeFolder["tone"] }> = {
  caches: { note: "App and package caches", tone: "blue" },
  logs: { note: "Logs and crash reports", tone: "teal" },
  developer: { note: "Xcode and simulators", tone: "indigo" },
  projects: { note: "Your projects, for node_modules (read-only)", tone: "green" },
  downloads: { note: "Large and old files, inspect only", tone: "slate" },
  desktop: { note: "Large and old files, inspect only", tone: "slate" },
  documents: { note: "Large and old files, inspect only", tone: "slate" },
  movies: { note: "Large and old files, inspect only", tone: "slate" },
  added: { note: "Added by you", tone: "orange" },
};

const accessNotes: Partial<Record<FolderAccess, string>> = {
  notRequested: "Skipped until you grant access",
  denied: "macOS didn’t allow access",
  missing: "Not on this Mac",
};

export function scopeFolders(folders: readonly ScopeFolderData[]): ScopeFolder[] {
  return folders.map((folder, index) => {
    const meta = folderNotes[folder.id];
    const protectedFolder = folder.id === "downloads" || folder.id === "desktop" || folder.id === "documents" || folder.id === "movies" ? folder.id : undefined;
    return {
      key: `${folder.id}:${index}`,
      path: folder.id === "projects" ? "~ (projects)" : folder.path,
      note: accessNotes[folder.access] ?? meta.note,
      access: folder.access,
      tone: (folder.access === "granted" || folder.access === "fullDiskAccess") && protectedFolder ? "green" : meta.tone,
      modes: folder.modes,
      protectedFolder,
      added: folder.id === "added" || undefined,
    };
  });
}

function itemsText(items: number, bytes: number): string {
  return `${plural(items, "item")} · ${formatGb(toGb(bytes))}`;
}

/** Every scan mode's card, with the running or finished scan's steps and results. */
export function modeDefinitions(input: {
  options: ScanOptions;
  folders: readonly ScopeFolder[];
  result: CleanupScanResult | null;
  review: ReviewList | null;
}): Record<ScanMode, ScanModeDefinition> {
  const build = (mode: ScanMode): ScanModeDefinition => {
    const text = modeText[mode];
    const steps = stepsFor(mode, input.options);
    const finished = input.result?.mode === mode ? input.result : null;
    const reads = input.folders.filter((folder) => folder.modes.includes(mode));
    const readable = reads.filter((folder) => folder.access === "available" || folder.access === "granted" || folder.access === "fullDiskAccess");
    const skipped = reads.filter((folder) => folder.access === "notRequested" || folder.access === "denied");
    const findings = (input.review?.findings ?? []).filter((finding) => finding.source === "scan");
    const eligible = findings.filter((finding) => finding.eligibility === "eligible");
    const total = findings.reduce((sum, finding) => sum + finding.bytes, 0);
    return {
      ...text,
      idleDetail:
        steps.length === 0
          ? "Choose at least one tool in the options menu."
          : `Reads ${plural(readable.length, "folder")}.${skipped.length ? ` ${skipped.map((folder) => folder.path).join(", ")} ${skipped.length === 1 ? "is" : "are"} skipped until you grant access.` : ""} Nothing is moved or deleted.`,
      steps: steps.map((step) => {
        const done = finished?.steps.find((entry) => entry.id === step);
        return { label: stepLabels[step], found: done ? itemsText(done.items, done.bytes) : "" };
      }),
      results: finished ? resultLines(findings, finished) : [],
      doneDetail: finished
        ? `${plural(findings.length, "item")}, ${formatGb(toGb(total))}. ${eligible.length} ${eligible.length === 1 ? "is" : "are"} eligible for quarantine. Nothing was changed.`
        : "",
    };
  };
  return { standard: build("standard"), deep: build("deep"), dev: build("dev") };
}

const categoryOrder: readonly FindingCategory[] = ["cache", "pkg", "logs", "xcode", "node", "build", "apps", "large", "old"];

function resultLines(findings: readonly ReviewFinding[], result: CleanupScanResult): ScanResultLine[] {
  const lines: ScanResultLine[] = [];
  for (const category of categoryOrder) {
    const inCategory = findings.filter((finding) => finding.category === category);
    if (inCategory.length === 0) {
      continue;
    }
    const eligible = inCategory.filter((finding) => finding.eligibility === "eligible").length;
    const inspectOnly = inCategory.every((finding) => finding.eligibility === "inspectOnly");
    lines.push({
      label: categoryMeta[category].label,
      count: plural(inCategory.length, "item"),
      size: formatGb(toGb(inCategory.reduce((sum, finding) => sum + finding.bytes, 0))),
      tag: inspectOnly ? "Inspect only" : eligible > 0 ? `${eligible} eligible` : "Needs a check",
      status: inspectOnly ? "muted" : eligible > 0 ? "ok" : "warn",
      tone: categoryMeta[category].tone,
    });
  }
  if (result.docker && (result.docker.status === "running" || result.docker.status === "notRunning")) {
    const bytes =
      result.docker.status === "running"
        ? result.docker.usage.imagesBytes + result.docker.usage.buildCacheBytes + result.docker.usage.volumesBytes + result.docker.usage.containersBytes
        : (result.docker.diskImageBytes ?? 0);
    lines.push({ label: "Docker", count: result.docker.status === "running" ? "running" : "not running", size: formatGb(toGb(bytes)), tag: "Inventory only", status: "muted", tone: "teal" });
  }
  return lines;
}

const riskText = { low: "Low", medium: "Medium", unknown: "Unknown" } as const;
const certaintyText = { high: "High", medium: "Medium", unknown: "Unknown" } as const;

export const whyNotText: Record<WhyNot, string> = {
  appRunning: "Quit the app and scan again to include this.",
  xcodeRunning: "Quit Xcode and scan again to include this.",
  simulatorRunning: "Quit the Simulator and scan again to include this.",
  noLockfile: "Kleari only offers node_modules when a lockfile next to it can install the same packages again.",
  activeProject: "Something in this project is open in an app (an editor, a terminal or a dev server), so Kleari leaves its build folders alone until it’s closed.",
  appleCache: "macOS and Apple’s apps use this while running. Kleari leaves it alone.",
  manageInXcode: "Remove simulators and runtimes in Xcode (Window › Devices and Simulators, or Settings › Components) so Xcode stays consistent.",
  userFile: "Large and old files are listed for you to look at in Finder. Kleari won’t move them.",
  otherVolume: "It’s on another disk than Kleari’s quarantine folder, and Kleari only moves items within one disk.",
  changedSinceScan: "It changed after the scan. Scan again to review it.",
};

const eligibilityLong: Record<Finding["eligibility"], string> = {
  eligible: "Eligible for quarantine",
  notNow: "Not eligible right now",
  needsCheck: "Not eligible until it’s checked",
  rescan: "Changed since the scan",
  inspectOnly: "Inspect only, never offered for quarantine",
};

function days(value: number | null): string {
  return plural(value ?? 0, "day");
}

export function evidenceText(entry: EvidenceEntry): string {
  switch (entry.code) {
    case "inCaches":
      return "In ~/Library/Caches, where apps keep data they can download again";
    case "appNotRunning":
      return "The app it belongs to isn’t open";
    case "appRunning":
      return `${entry.app ?? "Its app"} is open and may be writing to it`;
    case "noAppMatch":
      return "No app with this name is open; Kleari can’t say which app uses it";
    case "appleCache":
      return "macOS or an Apple app uses it while running";
    case "packageCache":
      return "A package manager’s download cache";
    case "refetchedOnInstall":
      return "The tool downloads it again on the next install";
    case "inDerivedData":
      return "Inside Xcode’s DerivedData folder";
    case "xcodeNotRunning":
      return "Xcode isn’t open";
    case "xcodeRunning":
      return "Xcode is open and may be using it";
    case "rebuildSlower":
      return "The next build takes longer while it’s made again";
    case "archive":
      return "An archive you made in Xcode for TestFlight or the App Store";
    case "losesDebugSymbols":
      return "Without it, crash reports from that build can’t be read";
    case "deviceSupport":
      return "Debug symbols Xcode copied from a device you connected";
    case "copiedAgainOnConnect":
      return "Xcode copies them again the next time you connect that device";
    case "simulatorCache":
      return "Caches the iOS Simulator rebuilds";
    case "simulatorRunning":
      return "The Simulator is open";
    case "simulatorData":
      return "A simulated device, with the apps and data inside it";
    case "sharedStore":
      return "Projects that use it need pnpm install before they run again";
    case "toolchain":
      return "Installed Rust toolchains";
    case "logOlderThan":
      return `Nothing written for ${days(entry.value)}`;
    case "tempOlderThan":
      return `Nothing changed for ${days(entry.value)}`;
    case "notWrittenSince":
      return "Apps make new logs and temporary files when they need them";
    case "lockfileFound":
      return "A lockfile next to it can install the same packages again";
    case "noLockfile":
      return "No lockfile was found, so a reinstall may get different packages";
    case "projectActive":
      return "Something in the project is open in an app";
    case "projectInactive":
      return "Nothing in the project is open in any app";
    case "buildOutput":
      return "Made by a build or an install";
    case "reinstallNeeded":
      return "Install the project’s dependencies again before it runs";
    case "largerThan":
      return `Larger than ${formatGb(toGb(entry.value ?? 0))}`;
    case "notOpenedFor":
      return `Not opened for ${days(entry.value)}`;
    case "notKnownCache":
      return "Not a known cache, so only you can decide";
    case "otherVolume":
      return "On another disk than Kleari’s quarantine folder";
    case "leftoverOfRemovedApp":
      return "Left behind by an app you removed with Kleari";
    case "leftoverNoApp":
      return "Named for an app that isn’t installed, and unchanged for a month or more";
    case "losesAppData":
      return "The app’s settings and data go with it if you reinstall it later";
    case "installerFile":
      return "An installer: apps run from where they were installed, not from this file";
    case "downloadAgain":
      return "You can download it again from the app’s maker";
  }
}

function reasonFor(finding: ReviewFinding): string {
  const codes = new Set(finding.evidence.map((entry) => entry.code));
  switch (finding.category) {
    case "cache":
      if (codes.has("appleCache")) {
        return "A cache macOS uses";
      }
      return finding.eligibility === "notNow" ? "App cache, but the app is open" : "App cache the app downloads again";
    case "pkg":
      return codes.has("toolchain") ? "Installed toolchains" : codes.has("sharedStore") ? "Shared package store" : "Package downloads the tool can fetch again";
    case "xcode":
      if (codes.has("archive")) {
        return "An app archive you made in Xcode";
      }
      if (codes.has("deviceSupport")) {
        return "Debug symbols Xcode can copy again";
      }
      if (codes.has("simulatorData")) {
        return "A simulator, managed in Xcode";
      }
      if (codes.has("simulatorCache")) {
        return "Simulator caches it rebuilds";
      }
      return finding.eligibility === "notNow" ? "Build output, but Xcode is open" : "Build output Xcode recreates on the next build";
    case "node":
      if (finding.eligibility === "notNow") {
        return "Dependencies of a project you’re working on";
      }
      return codes.has("lockfileFound") ? "Dependencies the lockfile can install again" : "Dependencies, lockfile not found";
    case "build":
      return finding.eligibility === "notNow" ? "Build output of a project you’re working on" : "Build output a build makes again";
    case "logs":
      return codes.has("tempOlderThan") ? "Temporary files left behind" : "Old logs and crash reports";
    case "large":
      return `Large ${fileKindLabel[finding.fileCategory ?? "other"]} to look over`;
    case "old":
      return `${capitalize(fileKindLabel[finding.fileCategory ?? "other"])} you haven’t opened in a while`;
    case "apps":
      return codes.has("installerFile")
        ? "An installer you’ve already used"
        : codes.has("leftoverOfRemovedApp")
          ? "Data left by an app you removed"
          : "Data left by an app that’s gone";
  }
}

/** What a large or old file is, by its extension; Kleari decides nothing from it. */
const fileKindLabel: Record<FileCategory, string> = {
  video: "video",
  image: "image",
  audio: "audio file",
  archive: "archive",
  diskImage: "disk image",
  document: "document",
  code: "code or build file",
  other: "file",
};

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function displayName(finding: ReviewFinding): string {
  if (finding.tool !== null) {
    return cacheMeta[finding.tool]?.name ?? finding.name;
  }
  return finding.name;
}

export function findingView(finding: ReviewFinding, nowMs: number): Finding {
  const modified = finding.modifiedMs === null ? "Not reported" : ago(finding.modifiedMs, nowMs);
  return {
    id: finding.id,
    name: displayName(finding),
    category: finding.category,
    path: finding.path,
    gb: toGb(finding.bytes),
    files: `${finding.lowerBound ? "≥ " : ""}${finding.files.toLocaleString("en-US")}`,
    modified: modified.charAt(0).toUpperCase() + modified.slice(1),
    reason: reasonFor(finding),
    certainty: certaintyText[finding.certainty],
    risk: riskText[finding.risk],
    eligibility: finding.eligibility,
    eligibilityLong: eligibilityLong[finding.eligibility],
    whyNot: finding.whyNot === null ? undefined : whyNotText[finding.whyNot],
    evidence: finding.evidence.map((entry) => ({ tone: entry.tone, text: evidenceText(entry) })),
  };
}

export function categoryCounts(findings: readonly Finding[]): Record<FindingCategory, number> {
  const counts: Record<FindingCategory, number> = { cache: 0, xcode: 0, pkg: 0, node: 0, build: 0, logs: 0, large: 0, old: 0, apps: 0 };
  for (const finding of findings) {
    counts[finding.category] += 1;
  }
  return counts;
}

function clock(ms: number, withSeconds = false): string {
  return new Date(ms).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", second: withSeconds ? "2-digit" : undefined });
}

/** "Today 14:12", "Yesterday 18:02" or "6 Oct, 09:15". */
export function when(ms: number, nowMs: number): string {
  const day = (time: number) => new Date(time).toDateString();
  if (day(ms) === day(nowMs)) {
    return `Today, ${clock(ms)}`;
  }
  if (day(ms) === day(nowMs - DAY_MS)) {
    return `Yesterday, ${clock(ms)}`;
  }
  return `${new Date(ms).toLocaleDateString("en-GB", { day: "numeric", month: "short" })}, ${clock(ms)}`;
}

export { clock };

export function reviewSubtitle(review: ReviewList | null, nowMs: number): string {
  if (!review || review.findings.length === 0) {
    return "Nothing to review yet. Run a scan in Cleanup, or send items from Storage or Developer.";
  }
  const eligible = review.findings.filter((finding) => finding.eligibility === "eligible");
  const scan = review.scan ? `From the ${review.scan.mode === "dev" ? "developer" : review.scan.mode} scan, ${when(review.scan.startedAtMs, nowMs).toLowerCase()}${review.scan.cancelled ? " (stopped early)" : ""}` : "Items you sent";
  return `${scan} · ${plural(review.findings.length, "item")}, ${plural(eligible.length, "eligible")}. Risk and eligibility are shown separately.`;
}

export function lastScanSummary(review: ReviewList | null, nowMs: number): LastScanSummary | null {
  if (!review || review.findings.length === 0) {
    return null;
  }
  const total = review.findings.reduce((sum, finding) => sum + finding.bytes, 0);
  const eligibleBytes = review.findings.filter((finding) => finding.eligibility === "eligible").reduce((sum, finding) => sum + finding.bytes, 0);
  const groups = categoryOrder
    .map((category) => ({ category, findings: review.findings.filter((finding) => finding.category === category) }))
    .filter((group) => group.findings.length > 0)
    .sort((a, b) => b.findings.reduce((sum, finding) => sum + finding.bytes, 0) - a.findings.reduce((sum, finding) => sum + finding.bytes, 0))
    .slice(0, 4);
  return {
    title: `${review.scan ? "Last scan found" : "Review holds"} ${formatGb(toGb(total))} in ${plural(review.findings.length, "item")}`,
    sub: `${review.scan ? `${when(review.scan.startedAtMs, nowMs)} · ` : ""}${formatGb(toGb(eligibleBytes))} is eligible for quarantine. Risk and eligibility are shown separately.`,
    totalGb: Number(toGb(total).toFixed(1)),
    categories: groups.map(({ category, findings }) => {
      const eligible = findings.filter((finding) => finding.eligibility === "eligible").length;
      const risks = new Set(findings.map((finding) => finding.risk));
      const risk = risks.size === 1 ? [...risks][0] : "medium";
      const inspectOnly = findings.every((finding) => finding.eligibility === "inspectOnly");
      return {
        name: categoryMeta[category].label,
        gb: Number(toGb(findings.reduce((sum, finding) => sum + finding.bytes, 0)).toFixed(1)),
        items: findings.length,
        tone: categoryMeta[category].tone,
        risk: `${riskText[risk]} risk`,
        riskStatus: risk === "low" ? "ok" : risk === "medium" ? "warn" : "muted",
        eligibility: inspectOnly ? "Inspect only" : eligible > 0 ? `${eligible} eligible` : "Needs a check",
        eligibilityStatus: inspectOnly ? "muted" : eligible > 0 ? "ok" : "warn",
      };
    }),
  };
}

/** What moving these categories means afterwards. */
export function consequences(categories: readonly FindingCategory[]): string {
  const parts: string[] = [];
  const has = (category: FindingCategory) => categories.includes(category);
  if (has("xcode")) {
    parts.push("Xcode rebuilds its data on your next build");
  }
  if (has("cache") || has("pkg")) {
    parts.push("apps and tools download caches again when they need them");
  }
  if (has("node") || has("build")) {
    parts.push("projects need an install or build before they run again");
  }
  if (has("logs")) {
    parts.push("apps start new logs when they need them");
  }
  if (has("apps")) {
    parts.push("removed apps’ data and used installers wait in Quarantine, so a reinstalled app can get its settings back");
  }
  const text = parts.join(", and ");
  return text ? text.charAt(0).toUpperCase() + text.slice(1) : "Restore any item from Quarantine if you need it back";
}

const originText: Record<QuarantineBatchView["origin"], string> = {
  standard: "From your standard scan",
  deep: "From your deep scan",
  dev: "From your developer scan",
  sent: "Sent from Storage or Developer",
};

function quarantineItem(item: QuarantineItemView): QuarantineItem {
  const meta = categoryMeta[item.category];
  return { id: item.id, name: item.name, from: item.from, gb: toGb(item.bytes), tone: meta.tone, icon: meta.icon, status: item.status, conflict: item.status === "conflict" || undefined, chooseFolder: item.chooseFolder || undefined, neededFolder: item.neededFolder };
}

/** Batches newest first; one made in the last 10 minutes is marked new. */
export function quarantineBatches(batches: readonly QuarantineBatchView[], nowMs: number): QuarantineBatch[] {
  return batches.map((batch) => ({
    id: batch.id,
    title: when(batch.createdMs, nowMs),
    sub: `${originText[batch.origin]} · ${plural(batch.items.length, "item")}`,
    isNew: nowMs - batch.createdMs < 10 * 60_000 || undefined,
    items: batch.items.map(quarantineItem),
  }));
}

/** How long the oldest held item has been in quarantine. */
export function oldestText(batches: readonly QuarantineBatchView[], nowMs: number): string {
  if (batches.length === 0) {
    return "—";
  }
  const oldest = Math.min(...batches.map((batch) => batch.createdMs));
  const age = Math.floor(Math.max(0, nowMs - oldest) / DAY_MS);
  return age < 1 ? "Today" : plural(age, "day");
}

/** Why a move didn't happen (the item stayed where it was). */
export const moveFailureText: Record<MoveFailure, string> = {
  needsAccess: "macOS refused access to its folder. Allow Kleari in System Settings › Privacy & Security, then try again.",
  missing: "it was no longer there.",
  changed: "it changed just before the move.",
  otherVolume: "it’s on another disk than the quarantine folder.",
  exists: "something with the same name was already in the quarantine folder.",
  folderUnavailable: "its folder couldn’t be opened safely.",
  quarantineUnavailable: "the quarantine folder couldn’t be opened safely.",
  failed: "macOS reported an error.",
};

export const historyReasonText: Record<HistoryUnavailableReason, string> = {
  cannotOpen: "Kleari couldn’t open its history file.",
  corrupt: "Kleari’s history file is damaged, so it was left as it is.",
  newerVersion: "A newer version of Kleari updated its history file. Use that version to manage quarantine.",
  migrationFailed: "Kleari couldn’t update its history file, so it was left as it is.",
  writeFailed: "Kleari couldn’t write to its history file. The disk may be full.",
};
