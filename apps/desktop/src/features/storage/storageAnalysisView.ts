// Turns the validated storage overview and analysis into what the v2 Storage page shows.
// Pure: every state (not analyzed, running, stopped, failed, unavailable) is decided here.

import type {
  AccessStatus,
  AnalysisNode,
  AnalysisProgress,
  AnalysisStepId,
  FileLock,
  StorageAnalysisResult,
  StorageCategoryId,
  StorageOverview,
} from "../../contracts/storageAnalysis";
import { analysisStepIds } from "../../contracts/storageAnalysis";
import type { StorageErrorKind } from "../../lib/storageIpc";
import { formatClockTime } from "../overview/format";
import { storageErrorText } from "./format";
import type { Loadable } from "../system/systemView";
import type {
  AnalysisStep,
  FolderNode,
  FolderTag,
  LargeFile,
  ManagedSpace,
  StorageAnalysis,
  StorageCapacity,
  StorageCategoryDetail,
} from "../../v2/screens/storage/types";
import type { IconName } from "../../v2/ui/icons";
import type { Status, Tone } from "../../v2/ui/components";
import { formatGb, plural } from "../../v2/ui/format";

/** Storage uses decimal units, as Finder does. */
const GB = 1_000_000_000;
const DAY_MS = 86_400_000;

const toGb = (bytes: number) => bytes / GB;

export interface AnalysisState {
  running: boolean;
  progress: AnalysisProgress | null;
  /** The last completed analysis; a cancelled one never replaces it. */
  result: StorageAnalysisResult | null;
  stopped: boolean;
  error: StorageErrorKind | null;
}

export const idleAnalysis: AnalysisState = { running: false, progress: null, result: null, stopped: false, error: null };

export interface StorageView {
  volume: string;
  capacity: StorageCapacity | null;
  /** Explains Available, or why capacity is missing. */
  capacityNote: string | null;
  tree: FolderNode | null;
  categories: StorageCategoryDetail[];
  largeFiles: LargeFile[];
  largeFilesNote: string;
  managed: ManagedSpace[];
  analysis: StorageAnalysis;
  analysisSteps: AnalysisStep[];
  /** When the shown result was taken, and what it couldn't read. */
  analyzedNote: string | null;
  fullDiskAccess: AccessStatus;
}

interface CategoryMeta {
  label: string;
  tone: Tone;
  icon: IconName;
  blurb: string;
  pill: string;
  pillStatus: Status;
  /** The Cleanup scan that reviews it. */
  cleanupMode?: "standard" | "dev";
}

const categoryMeta: Record<StorageCategoryId | "notMeasured", CategoryMeta> = {
  applications: {
    label: "Applications",
    tone: "blue",
    icon: "apps",
    blurb:
      "Your apps and the data they keep: app containers, plus support files and caches in folders named after an installed app or its publisher (a Google folder for Chrome, for example, which can include other Google products’ data). macOS’s Storage settings use their own rules, so their figure can differ. Kleari doesn’t uninstall apps.",
    pill: "Shown only",
    pillStatus: "muted",
  },
  developer: {
    label: "Developer files",
    tone: "indigo",
    icon: "code",
    blurb: "Package caches, build products, simulators and container disks, found by where they are. Docker data stays read-only.",
    pill: "Partly reviewable",
    pillStatus: "ok",
    cleanupMode: "dev",
  },
  user: {
    label: "User files",
    tone: "purple",
    icon: "user",
    blurb: "Your documents, downloads and media. Kleari never suggests these on its own.",
    pill: "Your files",
    pillStatus: "link",
  },
  system: {
    label: "System files",
    tone: "orange",
    icon: "gear",
    blurb: "macOS itself and the shared system folders. macOS protects and manages them, so Kleari only shows them.",
    pill: "Managed by macOS",
    pillStatus: "muted",
  },
  caches: {
    label: "Caches and temporary",
    tone: "yellow",
    icon: "trash",
    blurb: "App caches, logs and temporary files that apps can recreate.",
    pill: "Reviewable",
    pillStatus: "ok",
    cleanupMode: "standard",
  },
  other: {
    label: "Other",
    tone: "gray",
    icon: "dots",
    blurb: "App data and settings Kleari couldn’t match to an installed app, and anything no location rule names.",
    pill: "Shown only",
    pillStatus: "muted",
  },
  notMeasured: {
    label: "Not measured",
    tone: "slate",
    icon: "lock",
    blurb:
      "Used space Kleari didn’t count: folders it can’t read without Full Disk Access, other users’ files, cloud folders, and macOS’s own volumes such as swap, the sleep image and recovery.",
    pill: "Not measured",
    pillStatus: "muted",
  },
};

const stepLabels: Record<AnalysisStepId, string> = {
  applications: "Applications",
  home: "Your home folder",
  system: "System files",
  libraryOptVar: "Library, opt and var",
};

const lockText: Record<FileLock, { label: string; tip: string }> = {
  docker: { label: "Docker data", tip: "Docker manages this disk image. Change its size in Docker’s settings." },
  xcodeManaged: { label: "Xcode managed", tip: "Remove simulator runtimes in Xcode’s settings." },
  macos: { label: "Managed by macOS", tip: "Part of macOS. Kleari only shows it." },
};

function latest<T>(state: Loadable<T>): T | null {
  return state.status === "ready" ? state.value : state.status === "error" ? state.last : null;
}

function tagFor(category: StorageCategoryId, depth: number): FolderTag | undefined {
  if (category === "system" && depth >= 1) {
    return "macos";
  }
  // The home folder itself mixes categories; its folders are labelled, not the folder.
  return category === "user" && depth >= 2 ? "yours" : undefined;
}

function folderNode(node: AnalysisNode, depth: number): FolderNode {
  if (node.kind === "others") {
    return { name: "Other items", gb: toGb(node.bytes), tone: "gray", icon: "dots" };
  }
  const meta = categoryMeta[node.category];
  return {
    name: node.name,
    gb: toGb(node.bytes),
    tone: meta.tone,
    icon: meta.icon,
    tag: tagFor(node.category, depth),
    lowerBound: node.lowerBound || undefined,
    children: node.children?.map((child) => folderNode(child, depth + 1)),
  };
}

/** The drill-down tree: the volume, then one row per analysis step that found something. */
export function buildTree(volume: string, result: StorageAnalysisResult): { tree: FolderNode; positions: Partial<Record<AnalysisStepId, number>> } {
  const positions: Partial<Record<AnalysisStepId, number>> = {};
  const children: FolderNode[] = [];
  for (const step of result.steps) {
    if (step.node) {
      positions[step.id] = children.length;
      children.push(folderNode(step.node, 1));
    }
  }
  return {
    tree: {
      name: volume,
      gb: toGb(result.steps.reduce((total, step) => total + step.bytes, 0)),
      tone: "gray",
      icon: "database",
      lowerBound: result.steps.some((step) => step.lowerBound) || undefined,
      children,
    },
    positions,
  };
}

const browseStep: Partial<Record<StorageCategoryId, AnalysisStepId>> = {
  applications: "applications",
  user: "home",
  system: "system",
};

export function buildCategories(result: StorageAnalysisResult, usedBytes: number | null, positions: Partial<Record<AnalysisStepId, number>>): StorageCategoryDetail[] {
  const categories: StorageCategoryDetail[] = result.categories.map((category) => {
    const meta = categoryMeta[category.id];
    const step = browseStep[category.id];
    const position = step === undefined ? undefined : positions[step];
    return {
      id: category.id,
      label: meta.label,
      gb: toGb(category.bytes),
      lowerBound: category.lowerBound || undefined,
      tone: meta.tone,
      icon: meta.icon,
      blurb: meta.blurb,
      top: category.top.map((item) => ({ name: item.label, gb: toGb(item.bytes) })),
      pill: meta.pill,
      pillStatus: meta.pillStatus,
      browse: position === undefined ? undefined : [position],
      cleanupMode: meta.cleanupMode,
    };
  });
  const counted = result.categories.reduce((total, category) => total + category.bytes, 0);
  if (usedBytes !== null && usedBytes > counted) {
    const meta = categoryMeta.notMeasured;
    categories.push({ id: "notMeasured", ...meta, gb: toGb(usedBytes - counted), top: [] });
  }
  return categories;
}

export function formatAge(modifiedMs: number | null, nowMs: number): string {
  if (modifiedMs === null) {
    return "modification date not reported";
  }
  const days = Math.floor(Math.max(0, nowMs - modifiedMs) / DAY_MS);
  if (days < 1) {
    return "modified today";
  }
  if (days < 31) {
    return `modified ${plural(days, "day")} ago`;
  }
  if (days < 365) {
    return `modified ${plural(Math.floor(days / 30.44), "month")} ago`;
  }
  return `modified ${plural(Math.floor(days / 365.25), "year")} ago`;
}

export function buildLargeFiles(result: StorageAnalysisResult, nowMs: number): LargeFile[] {
  return result.largeFiles.map((file) => {
    const meta = categoryMeta[file.category];
    return {
      id: `${file.folder}/${file.name}`,
      name: file.name,
      path: file.folder,
      age: formatAge(file.modifiedMs, nowMs),
      gb: toGb(file.bytes),
      tone: meta.tone,
      icon: meta.icon,
      locked: file.lock === null ? undefined : lockText[file.lock],
      reviewId: file.reviewId,
    };
  });
}

export function formatElapsed(ms: number): string {
  const seconds = Math.max(1, Math.round(ms / 1000));
  if (seconds < 60) {
    return `${seconds} s`;
  }
  const rest = seconds % 60;
  return rest === 0 ? `${Math.floor(seconds / 60)} min` : `${Math.floor(seconds / 60)} min ${rest} s`;
}

function countedBytes(result: StorageAnalysisResult): number {
  return result.steps.reduce((total, step) => total + step.bytes, 0);
}

export function buildAnalysis(state: AnalysisState, usedBytes: number | null): { analysis: StorageAnalysis; steps: AnalysisStep[] } {
  const progress = state.running ? state.progress : null;
  const stepIndex = progress ? Math.max(0, analysisStepIds.indexOf(progress.step)) : 0;
  const counted = progress ? progress.countedBytes : state.result ? countedBytes(state.result) : 0;
  let percent: number | null = 100;
  if (state.running) {
    // Used space is known before the walk, so bytes counted is an honest measure of progress.
    // Without it there is no denominator, and only the step is shown.
    percent = usedBytes ? Math.min(99, Math.round((counted / usedBytes) * 100)) : null;
  }
  const steps = analysisStepIds.map((id, index): AnalysisStep => {
    if (state.running) {
      const finished = progress?.finishedSteps[index];
      return { label: stepLabels[id], gb: finished === undefined ? null : toGb(finished) };
    }
    const step = state.result?.steps.find((entry) => entry.id === id);
    return { label: stepLabels[id], gb: step ? toGb(step.bytes) : null, lowerBound: step?.lowerBound || undefined };
  });
  return {
    analysis: {
      running: state.running,
      stopped: !state.running && state.stopped,
      failed: !state.running && state.error ? storageErrorText[state.error] : null,
      percent,
      stepIndex,
      countedGb: toGb(counted),
      doneNote: state.result ? `${formatGb(toGb(countedBytes(state.result)))} counted in ${formatElapsed(state.result.durationMs)}` : null,
    },
    steps,
  };
}

function managedSpace(overview: StorageOverview | null): ManagedSpace[] {
  const value = (reading: StorageOverview["purgeableBytes"] | undefined) => (reading?.status === "available" ? toGb(reading.value) : null);
  return [
    { label: "Purgeable space", sub: "Estimate. macOS frees this on its own when needed", gb: value(overview?.purgeableBytes), tone: "blue", icon: "database" },
    { label: "Swap and sleep image", sub: "Used for memory and sleep", gb: value(overview?.swapAndSleepBytes), tone: "gray", icon: "swap" },
  ];
}

function capacityNote(overview: Loadable<StorageOverview>, capacity: StorageCapacity | null, purgeableGb: number | null): string | null {
  if (overview.status === "loading") {
    return null;
  }
  if (capacity === null) {
    return "macOS didn’t report this disk’s capacity.";
  }
  return purgeableGb === null ? null : `Available includes about ${formatGb(purgeableGb)} macOS can free on its own.`;
}

function analyzedNote(result: StorageAnalysisResult | null): string | null {
  if (!result) {
    return null;
  }
  const at = `Analyzed at ${formatClockTime(result.startedAtMs)}.`;
  const { unreadable, tooDeep, hardLinksSkipped } = result.coverage;
  const notCounted = unreadable + tooDeep + hardLinksSkipped;
  return notCounted === 0 ? at : `${at} ${plural(notCounted, "item")} couldn’t be read or counted, so sizes marked “at least” are minimums.`;
}

export function buildStorageView(input: { overview: Loadable<StorageOverview>; analysis: AnalysisState; nowMs: number }): StorageView {
  const overview = latest(input.overview);
  const volume = overview?.volume.status === "available" ? overview.volume.value : null;
  const capacity = volume ? { usedGb: toGb(volume.usedBytes), totalGb: toGb(volume.totalBytes), availableGb: toGb(volume.availableBytes) } : null;
  const purgeableGb = overview?.purgeableBytes.status === "available" ? toGb(overview.purgeableBytes.value) : null;
  const volumeName = volume?.name ?? "Startup disk";
  const result = input.analysis.result;
  const built = result ? buildTree(volumeName, result) : null;
  const { analysis, steps } = buildAnalysis(input.analysis, volume?.usedBytes ?? null);
  const truncated = result?.largeFilesTruncated ? " Showing the 50 largest." : "";
  return {
    volume: volumeName,
    capacity,
    capacityNote: capacityNote(input.overview, capacity, purgeableGb),
    tree: built?.tree ?? null,
    categories: result && built ? buildCategories(result, volume?.usedBytes ?? null, built.positions) : [],
    largeFiles: result ? buildLargeFiles(result, input.nowMs) : [],
    largeFilesNote: `Files of 1 GB or more, largest first. Folders Kleari couldn’t read aren’t included.${truncated}`,
    managed: managedSpace(overview),
    analysis,
    analysisSteps: steps,
    analyzedNote: analyzedNote(result),
    fullDiskAccess: overview?.fullDiskAccess ?? "unknown",
  };
}
