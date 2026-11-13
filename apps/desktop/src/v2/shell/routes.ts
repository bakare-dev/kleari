import { useEffect, useRef, useSyncExternalStore } from "react";
import type { IconName } from "../ui/icons";

// Hash-based destinations, as in the current app: navigation never starts an action.
export const v2Routes = [
  "dashboard",
  "cleanup",
  "review",
  "preview",
  "quarantine",
  "storage",
  "apps",
  "system",
  "developer",
  "processes",
  "mouse",
  "activity",
  "settings",
] as const;

export type V2Route = (typeof v2Routes)[number];

/** The destination a hash names, if `allowed` includes it; otherwise `fallback`. Anything after
 * a `/` is a detail for that page (such as a Settings section), not part of the destination. */
export function routeFromHash(hash: string, allowed: readonly V2Route[] = v2Routes, fallback: V2Route = "dashboard"): V2Route {
  const name = hash.replace(/^#/, "").split("/")[0];
  return allowed.find((route) => route === name) ?? fallback;
}

/** The detail after the destination in a hash (`scanning` in `#settings/scanning`); `null` if none. */
export function detailFromHash(hash: string): string | null {
  const [, detail] = hash.replace(/^#/, "").split("/");
  return detail || null;
}

export function hrefFor(route: V2Route, detail?: string): string {
  return detail ? `#${route}/${detail}` : `#${route}`;
}

export interface NavDestination {
  route: V2Route;
  label: string;
  icon: IconName;
}

/** Sidebar order from the handoff; Settings is pinned to the bottom. */
export const mainNav: readonly NavDestination[] = [
  { route: "dashboard", label: "Dashboard", icon: "dashboard" },
  { route: "cleanup", label: "Cleanup", icon: "search" },
  { route: "quarantine", label: "Quarantine", icon: "quarantine" },
  { route: "storage", label: "Storage", icon: "disk" },
  { route: "apps", label: "Apps", icon: "apps" },
  { route: "system", label: "System", icon: "pulse" },
  { route: "developer", label: "Developer", icon: "code" },
  { route: "processes", label: "Processes", icon: "list" },
  { route: "mouse", label: "Mouse", icon: "mouse" },
  { route: "activity", label: "Activity", icon: "clock" },
];

export const settingsNav: NavDestination = { route: "settings", label: "Settings", icon: "sliders" };

/** Review and Preview are steps 2 and 3 of Cleanup, so Cleanup stays selected. */
export function navRouteFor(route: V2Route): V2Route {
  return route === "review" || route === "preview" ? "cleanup" : route;
}

export function headingIdFor(route: V2Route): string {
  return `${route}-title`;
}

function subscribe(onChange: () => void) {
  window.addEventListener("hashchange", onChange);
  return () => window.removeEventListener("hashchange", onChange);
}

/** The current destination. Release builds pass only the destinations they ship. */
export function useV2Route(allowed: readonly V2Route[] = v2Routes, fallback: V2Route = "dashboard"): V2Route {
  return useSyncExternalStore(subscribe, () => routeFromHash(window.location.hash, allowed, fallback), () => fallback);
}

/** Focus the new page heading after navigation (not on first load). */
export function useFocusHeadingOnRouteChange(route: V2Route) {
  const previous = useRef(route);
  useEffect(() => {
    if (previous.current === route) {
      return;
    }
    previous.current = route;
    document.getElementById(headingIdFor(route))?.focus();
  }, [route]);
}

export function navigate(route: V2Route, detail?: string) {
  window.location.hash = hrefFor(route, detail);
}
