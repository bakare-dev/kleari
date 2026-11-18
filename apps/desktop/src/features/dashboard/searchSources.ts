import type { ActivityList } from "../../contracts/activity";
import type { AppsScan } from "../../contracts/apps";
import type { QuarantineList, ReviewList } from "../../contracts/cleanup";
import { tools, type FilesScan, type Tool } from "../../contracts/files";
import type { Task } from "../../contracts/maintenance";
import type { ProcessList } from "../../contracts/processes";
import type { StorageAnalysisResult } from "../../contracts/storageAnalysis";
import type { StartupItems } from "../../contracts/systemPage";
import { categoryMeta } from "../../v2/screens/cleanup/findingMeta";
import type { DevEntry } from "../../v2/screens/developer/DeveloperScreen";
import { toolMeta } from "../../v2/screens/files/FilesScreen";
import { taskMeta } from "../../v2/screens/maintenance/MaintenanceScreen";
import { sections } from "../../v2/screens/settings/SettingsScreen";
import { mainNav, type V2Route } from "../../v2/shell/routes";
import { formatGb } from "../../v2/ui/format";
import { activityDays } from "../activity/activityView";
import { buildDeveloperView, type DeveloperScanState } from "../developer/developerView";
import type { Loadable } from "../system/systemView";
import type { SearchItem, SearchSource } from "./search";

/** Words a page is also found by, besides its name. */
const pageWords: Record<V2Route, string> = {
  dashboard: "home overview readings",
  cleanup: "scan junk caches logs clean",
  review: "findings",
  preview: "",
  quarantine: "restore undo held trash bin",
  storage: "disk space analyze large files browse folders",
  apps: "uninstall applications leftovers installers",
  files: "duplicates similar images photos large old downloads ai clutter",
  maintenance: "ram memory dns spotlight snapshots purgeable login items caches",
  browser: "tabs chrome safari firefox edge brave",
  system: "cpu memory battery temperature fans health specs network internet download upload speed",
  developer: "xcode node_modules docker caches projects simulators",
  processes: "quit running apps cpu memory",
  mouse: "buttons scroll wheel shortcuts",
  activity: "log history",
  settings: "preferences options",
};

const gb = (bytes: number) => formatGb(bytes / 1e9);

/** What each module holds this session, for the Dashboard's search. Nothing here reads anything. */
export interface SearchInput {
  review: ReviewList | null;
  quarantine: QuarantineList | null;
  analysis: StorageAnalysisResult | null;
  apps: AppsScan | null;
  files: Partial<Record<Tool, FilesScan>>;
  maintenanceStartup: StartupItems | null;
  developer: DeveloperScanState;
  developerFolders: readonly string[];
  processes: Loadable<ProcessList>;
  activity: ActivityList | null;
  nowMs: number;
}

function pages(): SearchSource {
  return {
    id: "pages",
    label: "Pages",
    icon: "dashboard",
    tone: "gray",
    target: { route: "dashboard" },
    items: [...mainNav, { route: "settings" as const, label: "Settings", icon: "sliders" as const }].map((page) => ({
      id: page.route,
      title: page.label,
      sub: "Open the page",
      text: pageWords[page.route],
      icon: page.icon,
      target: { route: page.route },
    })),
  };
}

function findings(review: ReviewList | null): SearchSource {
  const none = review === null || (review.scan === null && review.findings.length === 0);
  return {
    id: "review",
    label: "Cleanup findings",
    icon: "search",
    tone: "blue",
    target: { route: "review", list: "review" },
    pending: "Run a Cleanup scan to search what it finds.",
    items: none
      ? null
      : review.findings.map((finding) => ({
          id: finding.id,
          title: finding.name,
          sub: `${categoryMeta[finding.category].label} · ${gb(finding.bytes)} · ${finding.location}`,
          text: finding.path,
          icon: categoryMeta[finding.category].icon,
          tone: categoryMeta[finding.category].tone,
        })),
  };
}

function quarantined(list: QuarantineList | null): SearchSource {
  return {
    id: "quarantine",
    label: "Quarantine",
    icon: "quarantine",
    tone: "orange",
    target: { route: "quarantine" },
    items:
      list?.status !== "available"
        ? null
        : list.batches.flatMap((batch) =>
            batch.items.map((item) => ({ id: `${batch.id}:${item.id}`, title: item.name, sub: `Held · ${gb(item.bytes)} · from ${item.from}`, text: item.from })),
          ),
  };
}

function storage(result: StorageAnalysisResult | null): SearchSource {
  return {
    id: "storage",
    label: "Storage",
    icon: "disk",
    tone: "green",
    target: { route: "storage", detail: "files" },
    pending: "Analyze in Storage to search its large files.",
    items: result === null ? null : result.largeFiles.map((file) => ({ id: file.reviewId, title: file.name, sub: `Large file · ${gb(file.bytes)} · ${file.folder}`, text: file.folder })),
  };
}

function apps(scan: AppsScan | null): SearchSource {
  const items: SearchItem[] = scan
    ? [
        ...scan.apps.map((app) => ({
          id: `app:${app.id}`,
          title: app.name,
          sub: `${app.apple ? "Part of macOS" : "App"} · ${gb(app.bytes + app.leftoverBytes)} · ${app.location}`,
          text: `${app.location} ${app.bundleId ?? ""}`,
        })),
        ...scan.orphans.map((left, index) => ({
          id: `left:${index}`,
          title: left.name,
          sub: `Left by ${left.owner} · ${gb(left.bytes)}`,
          text: `${left.owner} ${left.location}`,
          target: { route: "apps" as const, detail: "leftovers", list: "apps.leftovers" as const },
        })),
        ...scan.installers.map((file) => ({
          id: `inst:${file.reviewId}`,
          title: file.name,
          sub: `Installer · ${gb(file.bytes)} · ${file.location}`,
          text: file.location,
          target: { route: "apps" as const, detail: "installers", list: "apps.installers" as const },
        })),
      ]
    : [];
  return {
    id: "apps",
    label: "Apps",
    icon: "apps",
    tone: "blue",
    target: { route: "apps", detail: "apps", list: "apps" },
    pending: "Scan in Apps to search your apps.",
    items: scan ? items : null,
  };
}

function files(results: Partial<Record<Tool, FilesScan>>): SearchSource {
  const done = tools.filter((tool) => results[tool] !== undefined);
  const items = done.flatMap((tool) => {
    const seen = new Set<string>();
    return (results[tool]?.groups ?? []).flatMap((group) =>
      group.items.flatMap((item) => {
        if (seen.has(item.id)) {
          return [];
        }
        seen.add(item.id);
        return [{
          id: `${tool}:${item.id}`,
          title: item.name,
          sub: `${toolMeta[tool].label} · ${gb(item.bytes)} · ${item.location}`,
          text: item.location,
          icon: toolMeta[tool].icon,
          tone: toolMeta[tool].tone,
          target: { route: "files" as const, detail: tool, list: `files.${tool}` as const },
        }];
      }),
    );
  });
  return {
    id: "files",
    label: "Files",
    icon: "file",
    tone: "orange",
    target: { route: "files" },
    pending: "Search in Files to include what it finds.",
    items: done.length === 0 ? null : items,
  };
}

function maintenance(startup: StartupItems | null): SearchSource {
  const tasks = (Object.keys(taskMeta) as Task[]).map((task) => ({
    id: `task:${task}`,
    title: taskMeta[task].title,
    sub: "Maintenance task",
    text: taskMeta[task].what,
    icon: taskMeta[task].icon,
    tone: taskMeta[task].tone,
  }));
  const login = (startup?.items ?? []).map((item) => ({
    id: `login:${item.scope}:${item.label}`,
    title: item.name,
    sub: "Login or background item",
    text: item.label,
    icon: "power" as const,
    target: { route: "maintenance" as const, detail: "login" },
  }));
  const keyboard = { id: "keyboard", title: "Clean the keyboard", sub: "Maintenance task: lock every key for a while", text: "keyboard lock wipe", icon: "key" as const, tone: "blue" as const };
  return { id: "maintenance", label: "Maintenance", icon: "gear", tone: "gray", target: { route: "maintenance", detail: "tasks" }, items: [keyboard, ...tasks, ...login] };
}

function developer(state: DeveloperScanState, folders: readonly string[], nowMs: number): SearchSource {
  const view = state.result ? buildDeveloperView({ state, usedBytes: null, addedFolders: folders, nowMs }) : null;
  const entries = (list: readonly DevEntry[], detail: "toolchains" | "projects") =>
    list.map((entry) => ({
      id: `${detail}:${entry.id}`,
      title: entry.name,
      sub: `${detail === "projects" ? "Project" : "Developer cache"} · ${formatGb(entry.gb)} · ${entry.sub}`,
      text: `${entry.sub} ${entry.path}`,
      icon: entry.icon,
      tone: entry.tone,
      target: { route: "developer" as const, detail, list: detail === "projects" ? ("developer.projects" as const) : ("developer.toolchains" as const) },
    }));
  return {
    id: "developer",
    label: "Developer",
    icon: "code",
    tone: "indigo",
    target: { route: "developer", detail: "toolchains", list: "developer.toolchains" },
    pending: "Scan in Developer to search caches and projects.",
    items: view ? [...entries(view.toolchains, "toolchains"), ...entries(view.projects, "projects")] : null,
  };
}

function processes(state: Loadable<ProcessList>): SearchSource {
  const list = state.status === "ready" ? state.value : state.status === "error" ? state.last : null;
  return {
    id: "processes",
    label: "Processes",
    icon: "list",
    tone: "teal",
    target: { route: "processes" },
    items: list === null ? null : list.rows.map((row) => ({ id: row.id, title: row.name, sub: `Running · ${row.kind === "app" ? "app" : row.kind === "background" ? "background" : "system"}`, text: row.location ?? "" })),
  };
}

function activity(list: ActivityList | null, nowMs: number): SearchSource {
  return {
    id: "activity",
    label: "Activity",
    icon: "clock",
    tone: "gray",
    target: { route: "activity" },
    items:
      list?.status !== "available"
        ? null
        : activityDays(list.events, nowMs).flatMap((day) =>
            day.events.map((event) => ({ id: event.id, title: event.title, sub: `${day.label} ${event.time} · ${event.sub}`, text: event.lines.map((line) => line.name).join(" ") })),
          ),
  };
}

function settings(): SearchSource {
  return {
    id: "settings",
    label: "Settings",
    icon: "sliders",
    tone: "gray",
    target: { route: "settings" },
    items: sections.map((section) => ({ id: section.id, title: section.label, sub: section.blurb, icon: section.icon, tone: section.tone, target: { route: "settings" as const, detail: section.id } })),
  };
}

/** Every module the Dashboard searches, in the sidebar's order after Pages. */
export function searchSources(input: SearchInput): SearchSource[] {
  return [
    pages(),
    findings(input.review),
    quarantined(input.quarantine),
    storage(input.analysis),
    apps(input.apps),
    files(input.files),
    maintenance(input.maintenanceStartup),
    developer(input.developer, input.developerFolders, input.nowMs),
    processes(input.processes),
    activity(input.activity, input.nowMs),
    settings(),
  ];
}
