// What each issue alert says and where it leads, shared by the Dashboard and the menu-bar
// popover. Pure: no state, no IPC.

import type { Issue, IssueKind } from "../../contracts/issues";
import type { MenuBarDestination } from "../../lib/menubarIpc";
import type { Tone } from "../../v2/ui/components";
import { formatGb, plural } from "../../v2/ui/format";
import type { IconName } from "../../v2/ui/icons";

export interface IssueLook {
  icon: IconName;
  tone: Tone;
  route: MenuBarDestination;
  action: string;
}

export const issueLook: Record<IssueKind, IssueLook> = {
  lowDiskSpace: { icon: "disk", tone: "orange", route: "storage", action: "See what takes space" },
  criticalDiskSpace: { icon: "disk", tone: "red", route: "storage", action: "See what takes space" },
  lowDriveSpace: { icon: "disk", tone: "orange", route: "storage", action: "Open Storage" },
  heavyMemory: { icon: "chip", tone: "orange", route: "processes", action: "Open Processes" },
  criticalMemory: { icon: "chip", tone: "red", route: "processes", action: "Open Processes" },
  busyCpu: { icon: "pulse", tone: "orange", route: "processes", action: "See what is busy" },
  batteryDrain: { icon: "battery", tone: "orange", route: "processes", action: "See what is busy" },
  batteryLow: { icon: "battery", tone: "orange", route: "system", action: "Open System" },
  batteryCritical: { icon: "battery", tone: "red", route: "system", action: "Open System" },
  batteryService: { icon: "battery", tone: "red", route: "system", action: "Open System" },
  batteryCycles: { icon: "battery", tone: "yellow", route: "system", action: "Open System" },
  batteryHot: { icon: "battery", tone: "red", route: "system", action: "Open System" },
  lowDeviceBattery: { icon: "battery", tone: "yellow", route: "system", action: "Open System" },
  fullTrash: { icon: "trashLines", tone: "blue", route: "maintenance", action: "Empty in Maintenance" },
  unusedApps: { icon: "apps", tone: "violet", route: "apps", action: "Uninstall in Apps" },
  manyLoginItems: { icon: "power", tone: "indigo", route: "maintenance", action: "Review login items" },
  cleanupDue: { icon: "search", tone: "blue", route: "cleanup", action: "Scan now" },
};

const gb = (bytes: number | null) => formatGb((bytes ?? 0) / 1e9);

export function issueText(issue: Issue): { title: string; sub: string } {
  const value = issue.value ?? 0;
  switch (issue.kind) {
    case "lowDiskSpace":
      return { title: "Your startup disk is getting full", sub: `${gb(issue.value)} free of ${gb(issue.total)}` };
    case "criticalDiskSpace":
      return { title: "Your startup disk is almost full", sub: `Only ${gb(issue.value)} free of ${gb(issue.total)}` };
    case "lowDriveSpace":
      return { title: `${issue.subject ?? "A drive"} is almost full`, sub: `${gb(issue.value)} free of ${gb(issue.total)}` };
    case "heavyMemory":
      return { title: "Memory is under pressure", sub: "Apps may slow down" };
    case "criticalMemory":
      return { title: "Memory pressure is critical", sub: "Apps may stall" };
    case "busyCpu":
      return { title: "Your Mac has been busy for a while", sub: `About ${value}% of its processor for 15 minutes` };
    case "batteryDrain":
      return { title: "Your battery is draining fast", sub: `About ${issue.total ?? 0} W on battery` };
    case "batteryLow":
      return { title: "Battery low", sub: `${value}% left · plug in soon` };
    case "batteryCritical":
      return { title: "Battery critically low", sub: `${value}% left · plug in now` };
    case "batteryService":
      return { title: "macOS recommends battery service", sub: "System Settings › Battery › Battery Health" };
    case "batteryCycles":
      return { title: "Battery near the end of its design life", sub: `${value} of ${issue.total ?? 0} cycles` };
    case "batteryHot":
      return { title: "Your battery is hot", sub: `${value} °C · let your Mac cool` };
    case "lowDeviceBattery":
      return { title: `${issue.subject ?? "A Bluetooth device"} battery is low`, sub: `${value}% left` };
    case "fullTrash":
      return { title: "The Trash is getting large", sub: `${gb(issue.value)} in the Trash` };
    case "unusedApps":
      return { title: `${plural(value, "app")} not opened in 6 months`, sub: issue.subject ?? "" };
    case "manyLoginItems":
      return { title: "Many apps start at login", sub: `${value} login items and agents` };
    case "cleanupDue":
      return { title: "Time for a cleanup", sub: `No Cleanup scan in ${plural(value, "day")}` };
  }
}

/** Warnings first, then the rest, in the order Kleari found them. */
export function sortedIssues(issues: readonly Issue[]): Issue[] {
  return [...issues].sort((a, b) => (a.severity === b.severity ? 0 : a.severity === "warning" ? -1 : 1));
}
