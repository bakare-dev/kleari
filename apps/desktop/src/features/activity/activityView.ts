import type { ActivityEvent as EventData, ActivityLine as LineData } from "../../contracts/activity";
import type { ActivityDay, ActivityEvent, ActivityKind, ActivityLine, ActivityStat } from "../../v2/screens/activity/ActivityScreen";
import type { IconName } from "../../v2/ui/icons";
import { plural } from "../../v2/ui/format";
import { formatBytes, formatQuantity } from "../overview/format";
import { clock } from "../cleanup/cleanupView";

const DAY_MS = 86_400_000;
const WEEK_MS = 7 * DAY_MS;

const size = (bytes: number) => (bytes > 0 ? formatQuantity(formatBytes(bytes, "storage")) : "");

const scanNames: Record<string, string> = { standard: "Standard scan", deep: "Deep scan", dev: "Developer scan" };
const folderNames: Record<string, string> = { downloads: "Downloads", desktop: "Desktop", documents: "Documents", movies: "Movies" };

/** What happened to one item, as recorded with the event. */
const lineOutcome: Record<string, { note: string; tone?: ActivityLine["tone"] }> = {
  moved: { note: "Moved · restorable" },
  unconfirmed: { note: "Not confirmed yet", tone: "warn" },
  notMoved: { note: "Stayed in place", tone: "warn" },
  restored: { note: "Back in its original place", tone: "ok" },
  conflict: { note: "Something is in the way · nothing overwritten", tone: "warn" },
  folderMissing: { note: "Its folder is gone · kept in quarantine", tone: "warn" },
  missing: { note: "Not in the quarantine folder", tone: "warn" },
  needsAccess: { note: "macOS refused access · kept in quarantine", tone: "warn" },
  notAllowed: { note: "Its recorded place isn’t allowed · kept in quarantine", tone: "warn" },
  needsFolder: { note: "Its folder must be chosen first · kept in quarantine", tone: "warn" },
  deleted: { note: "Deleted · can’t be restored", tone: "danger" },
  kept: { note: "Not started · still in quarantine" },
  failed: { note: "Couldn’t be done · nothing changed", tone: "warn" },
};

function lines(event: EventData): ActivityLine[] {
  const shown = event.lines.map((line: LineData): ActivityLine => {
    const outcome = lineOutcome[line.outcome] ?? { note: "Outcome not recorded", tone: "warn" };
    return { name: line.name, path: line.folder || undefined, note: `${size(line.bytes)}${line.bytes > 0 ? " · " : ""}${outcome.note}`, tone: outcome.tone };
  });
  return event.moreLines > 0 ? [...shown, { name: plural(event.moreLines, "more item") }] : shown;
}

const stopped = (event: EventData) => event.outcome === "cancelled";

/** "Deep scan", or "Scheduled deep scan" for one the schedule ran. */
function scanTitle(event: EventData): string {
  const name = scanNames[event.subject ?? ""] ?? "Cleanup scan";
  return event.outcome === "scheduled" ? `Scheduled ${name.charAt(0).toLowerCase()}${name.slice(1)}` : name;
}

/** One event as the log shows it. Wording follows what was recorded, never more. */
export function activityEvent(event: EventData): ActivityEvent {
  const base = { id: event.id, time: clock(event.atMs), size: size(event.bytes), lines: lines(event) };
  const done = event.items - event.failed;
  const make = (kind: ActivityKind, icon: IconName, title: string, sub: string, link?: ActivityEvent["link"]): ActivityEvent => ({ ...base, kind, icon, title, sub, link });
  switch (event.kind) {
    case "scan":
      return make(
        "scan",
        "search",
        scanTitle(event),
        `${stopped(event) ? "Stopped early · " : "Read-only · "}${plural(event.items, "finding")}${stopped(event) ? " so far" : ""}`,
        { route: "review", label: "Open review" },
      );
    case "analysis":
      return make("scan", "disk", "Analyzed the startup disk", `${stopped(event) ? "Stopped early · " : "Read-only · "}${size(event.bytes) || "nothing"} counted · nothing changed`, { route: "storage", label: "Open Storage" });
    case "developerScan":
      return make("scan", "code", "Scanned developer files", `${stopped(event) ? "Stopped early · " : "Read-only · "}${plural(event.items, "cache or project", "caches and projects")}`, { route: "developer", label: "Open Developer" });
    case "review":
      return make(
        "review",
        "send",
        `Sent ${plural(event.items, "item")} to review`,
        `From ${reviewSources[event.subject ?? ""] ?? "Developer"} · nothing moved`,
        { route: "review", label: "Open review" },
      );
    case "quarantine":
      return make(
        "quarantine",
        "quarantine",
        done > 0 ? `Moved ${plural(done, "item")} to quarantine` : "Nothing moved to quarantine",
        event.failed > 0 && done > 0 ? `After you approved the preview · ${event.failed} stayed in place` : done > 0 ? "After you approved the preview · restorable" : "Every item stayed in place",
        { route: "quarantine", label: "Open Quarantine" },
      );
    case "restore": {
      const blocked = done === 0;
      return make(
        blocked ? "blocked" : "restore",
        blocked ? "warning" : "restore",
        blocked ? `Restore blocked for ${plural(event.items, "item")}` : `Restored ${plural(done, "item")}`,
        blocked ? "Kept in quarantine · nothing overwritten" : event.failed > 0 ? `${event.failed} kept in quarantine` : "Back in their original places",
        { route: "quarantine", label: "Open Quarantine" },
      );
    }
    case "delete":
      return make(
        "delete",
        "trashLines",
        done > 0 ? `Deleted ${plural(done, "item")} permanently` : "Nothing deleted",
        event.outcome === "stopped"
          ? `From quarantine · you stopped it; ${plural(event.failed, "item")} still in quarantine`
          : event.failed > 0
            ? `From quarantine · ${event.failed} couldn’t be deleted and stay held`
            : "From quarantine · you confirmed it",
      );
    case "access": {
      const folder = folderNames[event.subject ?? ""] ?? "a folder";
      return event.outcome === "granted"
        ? make("system", "key", `Access to ${folder} allowed`, "Read only · you chose Grant access")
        : make("system", "key", `Access to ${folder} refused`, "macOS kept it private · change it in System Settings");
    }
    case "quit": {
      const app = event.subject ?? "An app";
      return event.outcome === "requested"
        ? make("system", "close", `Asked ${app} to quit`, "From Processes · the app decides, and may ask to save")
        : make("system", "close", `Couldn’t ask ${app} to quit`, "From Processes · macOS didn’t deliver the request");
    }
    case "maintenance": {
      const task = maintenanceNames[event.subject ?? ""] ?? "A maintenance task";
      return event.outcome === "completed"
        ? make("system", "gear", `${task}: done`, "From Maintenance · you confirmed it")
        : event.outcome === "cancelled"
          ? make("system", "gear", `${task}: cancelled`, "From Maintenance · nothing was changed")
          : make("system", "gear", `${task}: didn’t finish`, "From Maintenance · macOS reported a problem", { route: "maintenance", label: "Open Maintenance" });
    }
    case "appRemoved":
      return make("system", "apps", `Moved ${event.subject ?? "an app"} to the Trash`, "From Apps · Finder’s Put Back returns it", { route: "apps", label: "Open Apps" });
  }
}

/** Maintenance tasks' names, by task ID. */
const maintenanceNames: Record<string, string> = {
  freeRam: "Free up memory",
  flushDns: "Flush the DNS cache",
  thinSnapshots: "Free up purgeable space",
  reindexSpotlight: "Reindex Spotlight",
  clearSystemCaches: "Clear system caches",
  clearSystemLogs: "Clear old system logs",
  removeDeletedUsers: "Remove deleted users",
  emptyTrash: "Empty the Trash",
};

/** Where items sent to review came from, by the event's subject. */
const reviewSources: Record<string, string> = { storage: "Storage", developer: "Developer", apps: "Apps", files: "Files", maintenance: "Maintenance" };

function dayLabel(ms: number, nowMs: number): { label: string; sub: string } {
  const date = new Date(ms);
  const sameDay = (other: number) => new Date(other).toDateString() === date.toDateString();
  const full = date.toLocaleDateString("en-GB", { weekday: "long", day: "numeric", month: "long" });
  if (sameDay(nowMs)) {
    return { label: "Today", sub: full };
  }
  if (sameDay(nowMs - DAY_MS)) {
    return { label: "Yesterday", sub: full };
  }
  return { label: date.toLocaleDateString("en-GB", { day: "numeric", month: "long" }), sub: date.toLocaleDateString("en-GB", { weekday: "long" }) };
}

/** Events grouped by local day, newest first (as they arrive). */
export function activityDays(events: readonly EventData[], nowMs: number): ActivityDay[] {
  const days: { key: string; label: string; sub: string; events: ActivityEvent[] }[] = [];
  for (const event of events) {
    const key = new Date(event.atMs).toDateString();
    let day = days[days.length - 1];
    if (!day || day.key !== key) {
      day = { key, ...dayLabel(event.atMs, nowMs), events: [] };
      days.push(day);
    }
    day.events.push(activityEvent(event));
  }
  return days.map(({ label, sub, events: dayEvents }) => ({ label, sub, events: dayEvents }));
}

/** The last seven days, from what was recorded. */
export function activityStats(events: readonly EventData[], nowMs: number): ActivityStat[] {
  const week = events.filter((event) => event.atMs >= nowMs - WEEK_MS);
  const of = (kind: EventData["kind"]) => week.filter((event) => event.kind === kind);
  const scans = week.filter((event) => event.kind === "scan" || event.kind === "analysis" || event.kind === "developerScan");
  const moved = of("quarantine");
  const movedItems = moved.reduce((total, event) => total + event.items - event.failed, 0);
  const movedBytes = moved.reduce((total, event) => total + event.bytes, 0);
  const restores = of("restore");
  const restored = restores.reduce((total, event) => total + event.items - event.failed, 0);
  const blocked = restores.reduce((total, event) => total + event.failed, 0);
  const deletes = of("delete");
  const deletedBytes = deletes.reduce((total, event) => total + event.bytes, 0);
  const deletedItems = deletes.reduce((total, event) => total + event.items - event.failed, 0);
  return [
    { label: "Scans this week", value: String(scans.length), sub: "All read-only", tone: "blue", icon: "search" },
    { label: "Moved to quarantine", value: size(movedBytes) || "None", sub: movedItems > 0 ? `${plural(movedItems, "item")} · restorable while held` : "Nothing moved this week", tone: "orange", icon: "quarantine" },
    { label: "Restored", value: plural(restored, "item"), sub: blocked > 0 ? `Plus ${blocked} kept in quarantine` : "Nothing overwritten", tone: "green", icon: "restore" },
    { label: "Deleted for good", value: size(deletedBytes) || "None", sub: deletedItems > 0 ? `${plural(deletedItems, "item")} · each one confirmed` : "Only after you confirm", tone: "red", icon: "trashLines" },
  ];
}
