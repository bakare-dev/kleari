// Turns the validated Developer scan into what the Developer page shows. Pure: names,
// explanations, risk labels and every scan state are decided here.

import type { CacheResult, DeveloperProgress, DeveloperScan, DockerState, MeasuredPart, ProjectResult, ProjectStack } from "../../contracts/developer";
import type { StorageErrorKind } from "../../lib/storageIpc";
import { formatClockTime } from "../overview/format";
import { storageErrorText } from "../storage/format";
import { formatElapsed } from "../storage/storageAnalysisView";
import type { DevEntry, DevRisk, DockerView } from "../../v2/screens/developer/DeveloperScreen";
import type { IconName } from "../../v2/ui/icons";
import type { Tone } from "../../v2/ui/components";
import { plural } from "../../v2/ui/format";

const GB = 1_000_000_000;
const DAY_MS = 86_400_000;
/** A project committed to within this many days is active and never suggested. */
export const ACTIVE_DAYS = 14;

const toGb = (bytes: number) => bytes / GB;

interface CacheMeta {
  name: string;
  /** The tool a version belongs to, as in "npm 11.11.1". */
  tool: string;
  tone: Tone;
  icon: IconName;
  risk: DevRisk;
  what: string;
  after: string;
}

export const cacheMeta: Record<string, CacheMeta> = {
  xcodeDerivedData: { name: "Xcode DerivedData", tool: "Xcode", tone: "indigo", icon: "code", risk: "low", what: "Build products and indexes Xcode makes for each project.", after: "Xcode rebuilds it the next time you build. The first build is slower." },
  xcodeArchives: { name: "Xcode Archives", tool: "Xcode", tone: "purple", icon: "archive", risk: "check", what: "Builds you archived for TestFlight or the App Store.", after: "You lose the debug symbols needed to read crash reports from those builds." },
  xcodeDeviceSupport: { name: "iOS DeviceSupport", tool: "Xcode", tone: "blue", icon: "phone", risk: "low", what: "Debug symbols copied from devices you plugged in.", after: "Xcode copies them again the next time you connect that device." },
  simulatorDevices: { name: "Simulator devices", tool: "Xcode", tone: "gray", icon: "phone", risk: "xcode", what: "The simulated iPhones and iPads Xcode made, with the apps and data inside them.", after: "Apps and data saved inside those simulators are gone. Manage them in Xcode › Devices and Simulators." },
  simulatorRuntimes: { name: "Simulator runtimes", tool: "Xcode", tone: "blue", icon: "phone", risk: "xcode", what: "Full iOS system images that simulators run on.", after: "Xcode would have to download them again. Remove them in Xcode › Settings › Components." },
  homebrew: { name: "Homebrew cache", tool: "Homebrew", tone: "orange", icon: "mug", risk: "low", what: "Downloaded installers for formulae and casks.", after: "Nothing changes for installed tools. Reinstalling downloads them again." },
  npm: { name: "npm cache", tool: "npm", tone: "red", icon: "package", risk: "low", what: "Packages npm downloaded before, kept to install faster.", after: "Your next npm install downloads packages again." },
  pnpmStore: { name: "pnpm store", tool: "pnpm", tone: "yellow", icon: "layers", risk: "check", what: "One shared copy of packages that pnpm projects link to.", after: "Projects using it need pnpm install before they run again." },
  pnpmCache: { name: "pnpm cache", tool: "pnpm", tone: "yellow", icon: "package", risk: "low", what: "Package metadata pnpm downloaded.", after: "pnpm downloads it again when needed." },
  yarn: { name: "Yarn cache", tool: "Yarn", tone: "blue", icon: "package", risk: "low", what: "Packages Yarn downloaded before.", after: "Your next yarn install downloads packages again." },
  bun: { name: "Bun cache", tool: "Bun", tone: "pink", icon: "package", risk: "low", what: "Packages Bun downloaded before.", after: "Your next bun install downloads packages again." },
  gradle: { name: "Gradle caches", tool: "Gradle", tone: "green", icon: "layers", risk: "low", what: "Downloaded dependencies and build caches for Gradle projects.", after: "The next Gradle build downloads what it needs again." },
  maven: { name: "Maven repository", tool: "Maven", tone: "orange", icon: "layers", risk: "low", what: "Dependencies Maven downloaded for your Java projects.", after: "The next Maven build downloads what it needs again." },
  cocoapods: { name: "CocoaPods cache", tool: "CocoaPods", tone: "pink", icon: "package", risk: "low", what: "Downloaded pod sources.", after: "The next pod install downloads them again." },
  pip: { name: "pip cache", tool: "pip", tone: "teal", icon: "package", risk: "low", what: "Downloaded Python wheels.", after: "pip downloads them again when needed." },
  cargo: { name: "Cargo registry", tool: "Cargo", tone: "orange", icon: "package", risk: "low", what: "Crates Cargo downloaded for your Rust projects.", after: "The next cargo build downloads what it needs again." },
  rustup: { name: "Rust toolchains", tool: "rustup", tone: "orange", icon: "code", risk: "check", what: "Installed Rust compilers and their standard libraries.", after: "Projects that need a removed toolchain can't build until rustup installs it again." },
  goModules: { name: "Go modules", tool: "Go", tone: "teal", icon: "package", risk: "low", what: "Modules Go downloaded for your projects.", after: "The next go build downloads what it needs again." },
  goBuild: { name: "Go build cache", tool: "Go", tone: "teal", icon: "layers", risk: "low", what: "Compiled packages Go keeps to build faster.", after: "The next go build compiles them again. It is slower once." },
};

const unknownCache: CacheMeta = { name: "Developer cache", tool: "", tone: "gray", icon: "folder", risk: "check", what: "A developer folder.", after: "Check what uses it before removing anything." };

interface StackMeta {
  label: string;
  tone: Tone;
  what: string;
  after: string;
}

const stackMeta: Record<ProjectStack, StackMeta> = {
  node: { label: "Node.js", tone: "green", what: "Installed packages and build output.", after: "Installing packages and building again brings them back. Your source code is untouched." },
  nextJs: { label: "Next.js", tone: "indigo", what: "Installed packages and the Next.js build folder.", after: "Installing packages and building again brings them back. Your source code is untouched." },
  rust: { label: "Rust", tone: "orange", what: "Compiled output from cargo build.", after: "cargo build compiles it again. Your source code is untouched." },
  maven: { label: "Maven", tone: "orange", what: "Compiled output from Maven.", after: "The next Maven build makes it again. Your source code is untouched." },
  gradle: { label: "Gradle", tone: "green", what: "Gradle build output and caches.", after: "The next Gradle build makes them again. Your source code is untouched." },
  python: { label: "Python", tone: "yellow", what: "A virtual environment and tool caches.", after: "Recreate the environment from your requirements file. Check that one exists first." },
  cocoaPods: { label: "CocoaPods", tone: "pink", what: "Installed pods.", after: "pod install brings them back." },
  flutter: { label: "Flutter", tone: "teal", what: "Compiled app output and Dart tool files.", after: "flutter build makes them again." },
  php: { label: "PHP", tone: "purple", what: "Installed Composer packages.", after: "composer install brings them back." },
};

/** Build folders that can hold things a build doesn't make, so they are checked first. */
const CHECK_FOLDERS = new Set(["dist", "build", "out", ".venv", "venv", "vendor"]);

export function ago(ms: number, nowMs: number): string {
  const days = Math.floor(Math.max(0, nowMs - ms) / DAY_MS);
  if (days < 1) {
    return "today";
  }
  if (days < 31) {
    return `${plural(days, "day")} ago`;
  }
  if (days < 365) {
    return `${plural(Math.floor(days / 30.44), "month")} ago`;
  }
  return `${plural(Math.floor(days / 365.25), "year")} ago`;
}

function parts(list: readonly MeasuredPart[]) {
  return list.map((part) => ({ name: part.name === "" ? plural(part.items, "other item") : part.name, gb: toGb(part.bytes) }));
}

export function cacheEntry(cache: CacheResult): DevEntry {
  const meta = cacheMeta[cache.id] ?? unknownCache;
  return {
    id: cache.id,
    name: meta.name,
    sub: cache.version ? `${meta.tool} ${cache.version}` : (cache.locations[0] ?? ""),
    gb: toGb(cache.measured.bytes),
    lowerBound: cache.measured.lowerBound,
    risk: meta.risk,
    tone: meta.tone,
    icon: meta.icon,
    path: cache.locations.join(", "),
    meta: plural(cache.measured.files, "file"),
    what: meta.what,
    after: meta.after,
    parts: parts(cache.measured.parts),
    reviewIds: cache.reviewId === null ? [] : [cache.reviewId],
  };
}

export function projectRisk(project: ProjectResult, nowMs: number): DevRisk {
  if (project.lastCommitMs !== null && nowMs - project.lastCommitMs < ACTIVE_DAYS * DAY_MS) {
    return "active";
  }
  return project.stack === "python" || project.folders.some((folder) => CHECK_FOLDERS.has(folder.name)) ? "check" : "low";
}

export function projectEntry(project: ProjectResult, nowMs: number): DevEntry {
  const meta = stackMeta[project.stack];
  const risk = projectRisk(project, nowMs);
  const folders = project.folders.map((folder) => `${folder.name}/`).join(", ");
  const commit = project.lastCommitMs === null ? "no Git history" : `last commit ${ago(project.lastCommitMs, nowMs)}`;
  return {
    id: `project:${project.location}`,
    name: project.name,
    sub: `${folders} · ${commit}`,
    gb: toGb(project.bytes),
    lowerBound: project.folders.some((folder) => folder.measured.lowerBound) || undefined,
    risk,
    tone: meta.tone,
    icon: "git",
    path: project.location,
    meta: meta.label,
    what: meta.what,
    after: risk === "active" ? `Not suggested: you committed to it within the last ${ACTIVE_DAYS} days.` : meta.after,
    parts: project.folders.map((folder) => ({ name: folder.name, gb: toGb(folder.measured.bytes) })),
    // A project sends each of its build folders the backend could identify.
    reviewIds: project.folders.flatMap((folder) => (folder.reviewId === null ? [] : [folder.reviewId])),
  };
}

export function dockerView(state: DockerState): DockerView {
  if (state.status === "notInstalled" || state.status === "notChecked") {
    return { status: state.status };
  }
  if (state.status === "notRunning") {
    return { status: "notRunning", imageGb: state.diskImageBytes === null ? null : toGb(state.diskImageBytes) };
  }
  const { usage } = state;
  const entries: { label: string; bytes: number; tone: Tone }[] = [
    { label: "Images", bytes: usage.imagesBytes, tone: "teal" },
    { label: "Build cache", bytes: usage.buildCacheBytes, tone: "blue" },
    { label: "Volumes", bytes: usage.volumesBytes, tone: "indigo" },
    { label: "Containers", bytes: usage.containersBytes, tone: "yellow" },
  ];
  return {
    status: "running",
    totalGb: toGb(entries.reduce((total, entry) => total + entry.bytes, 0)),
    complete: usage.complete,
    imageGb: state.diskImageBytes === null ? null : toGb(state.diskImageBytes),
    parts: entries.map((entry) => ({ label: entry.label, gb: toGb(entry.bytes), tone: entry.tone })),
  };
}

function dockerBytes(state: DockerState): number {
  if (state.status === "running") {
    const { usage } = state;
    return usage.imagesBytes + usage.buildCacheBytes + usage.volumesBytes + usage.containersBytes;
  }
  return state.status === "notRunning" ? (state.diskImageBytes ?? 0) : 0;
}

export interface DeveloperScanState {
  running: boolean;
  progress: DeveloperProgress | null;
  /** The last completed scan; a cancelled one never replaces it. */
  result: DeveloperScan | null;
  stopped: boolean;
  error: StorageErrorKind | null;
}

export const idleDeveloperScan: DeveloperScanState = { running: false, progress: null, result: null, stopped: false, error: null };

export interface DeveloperView {
  toolchains: DevEntry[];
  projects: DevEntry[];
  developerGb: number | null;
  /** Some part of the total is a minimum, so the total and its share are too. */
  developerLowerBound: boolean;
  shareOfUsed: string | null;
  rebuildsGb: number | null;
  projectsGb: number | null;
  /** Some projects were missed or not fully measured. */
  projectsLowerBound: boolean;
  projectsNote: string;
  docker: DockerView | null;
  status: { running: boolean; text: string | null; failed: string | null; note: string | null };
}

function progressText(progress: DeveloperProgress | null): string {
  if (!progress) {
    return "Starting…";
  }
  switch (progress.phase) {
    case "caches":
      return `Measuring developer caches… (${progress.done} checked)`;
    case "projects":
      return `Measuring project build folders… (${progress.done} so far)`;
    case "docker":
      return "Asking Docker…";
  }
}

export function buildDeveloperView(input: { state: DeveloperScanState; usedBytes: number | null; addedFolders: readonly string[]; nowMs: number }): DeveloperView {
  const { state, nowMs } = input;
  const result = state.result;
  const toolchains = result ? result.caches.map(cacheEntry) : [];
  const projects = result ? result.projects.map((project) => projectEntry(project, nowMs)) : [];
  const all = [...toolchains, ...projects];
  const total = result ? all.reduce((sum, entry) => sum + entry.gb, 0) + toGb(dockerBytes(result.docker)) : null;
  // Projects missed by the search (a location skipped, folders unreadable, the 500 cap) make
  // the project total a minimum, and with it the overall total.
  const searchIncomplete = result !== null && (result.skippedRoots.length > 0 || result.unreadableFolders > 0 || result.projectsTruncated);
  const projectsLowerBound = searchIncomplete || projects.some((entry) => entry.lowerBound === true);
  const lowerBound = projectsLowerBound || toolchains.some((entry) => entry.lowerBound === true) || (result?.docker.status === "running" && !result.docker.usage.complete);
  const roots = result?.roots.join(", ") ?? ["~", ...input.addedFolders].join(", ");
  const unreadable = result && result.unreadableFolders > 0 ? ` ${plural(result.unreadableFolders, "folder")} couldn’t be searched.` : "";
  const skipped = result && result.skippedRoots.length > 0 ? ` Couldn’t search ${result.skippedRoots.join(", ")}.` : "";
  const truncated = result?.projectsTruncated ? " Only the first 500 projects were measured." : "";
  return {
    toolchains,
    projects,
    developerGb: total,
    developerLowerBound: lowerBound,
    shareOfUsed: total !== null && input.usedBytes ? `${lowerBound ? "at least " : ""}${Math.round(((total * GB) / input.usedBytes) * 100)}%` : null,
    rebuildsGb: result ? all.filter((entry) => entry.risk === "low").reduce((sum, entry) => sum + entry.gb, 0) : null,
    projectsGb: result ? projects.reduce((sum, entry) => sum + entry.gb, 0) : null,
    projectsLowerBound,
    projectsNote: `${plural(projects.length, "project")} in ${roots}`,
    docker: result ? dockerView(result.docker) : null,
    status: {
      running: state.running,
      text: state.running ? progressText(state.progress) : state.stopped ? "Scan stopped. Showing the last complete scan." : null,
      failed: !state.running && state.error ? storageErrorText[state.error] : null,
      note: result ? `Scanned at ${formatClockTime(result.startedAtMs)} in ${formatElapsed(result.durationMs)}.${skipped}${unreadable}${truncated}` : null,
    },
  };
}
