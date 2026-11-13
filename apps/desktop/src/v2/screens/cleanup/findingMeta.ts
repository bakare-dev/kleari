import type { IconName } from "../../ui/icons";
import type { Status, Tone } from "../../ui/components";
import type { Eligibility, FindingCategory, Level } from "./types";

export const categoryMeta: Record<FindingCategory, { label: string; tone: Tone; icon: IconName }> = {
  cache: { label: "App caches", tone: "blue", icon: "trash" },
  xcode: { label: "Xcode", tone: "indigo", icon: "code" },
  pkg: { label: "Package caches", tone: "teal", icon: "package" },
  node: { label: "node_modules", tone: "violet", icon: "hexagon" },
  build: { label: "Build folders", tone: "green", icon: "layers" },
  logs: { label: "Logs and temp", tone: "teal", icon: "clock" },
  large: { label: "Large files", tone: "orange", icon: "file" },
  old: { label: "Old files", tone: "yellow", icon: "archive" },
  apps: { label: "App leftovers", tone: "pink", icon: "apps" },
  chosen: { label: "Chosen by you", tone: "orange", icon: "folder" },
  broken: { label: "Broken items", tone: "red", icon: "warning" },
  startup: { label: "Login items", tone: "indigo", icon: "power" },
};

export const riskStatus: Record<Level, Status> = { Low: "ok", Medium: "warn", Unknown: "muted" };

export const eligibilityLabel: Record<Eligibility, string> = {
  eligible: "Eligible",
  notNow: "Not now",
  needsCheck: "Needs check",
  rescan: "Rescan",
  inspectOnly: "Inspect only",
};

export const eligibilityIcon: Record<Eligibility, IconName> = {
  eligible: "check",
  notNow: "blocked",
  needsCheck: "blocked",
  rescan: "clock",
  inspectOnly: "eye",
};
