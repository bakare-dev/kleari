import { useCallback, useEffect, useMemo, useReducer, useRef, useState, useSyncExternalStore } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { SystemMetrics } from "../../contracts/systemMetrics";
import { fetchSystemMetrics, MetricsError, type MetricsErrorKind } from "../../lib/systemMetricsIpc";
import { singleFlight, startPolling, type Poller } from "./metricsPoller";

export const REFRESH_INTERVAL_MS = 5_000;

export type MetricsState =
  | { status: "loading" }
  | { status: "ready"; metrics: SystemMetrics }
  | { status: "error"; error: MetricsErrorKind; lastMetrics: SystemMetrics | null };

export type MetricsEvent =
  | { type: "success"; metrics: SystemMetrics }
  | { type: "failure"; error: MetricsErrorKind };

export function reduceMetricsState(state: MetricsState, event: MetricsEvent): MetricsState {
  if (event.type === "success") {
    return { status: "ready", metrics: event.metrics };
  }
  const lastMetrics =
    state.status === "ready" ? state.metrics : state.status === "error" ? state.lastMetrics : null;
  return { status: "error", error: event.error, lastMetrics };
}

export function toErrorKind(error: unknown): MetricsErrorKind {
  return error instanceof MetricsError ? error.kind : "unknown";
}

function subscribeToVisibility(onChange: () => void) {
  document.addEventListener("visibilitychange", onChange);
  return () => document.removeEventListener("visibilitychange", onChange);
}

/** Foreground-only: polling stops while the window is hidden and resumes on return. */
export function useDocumentVisible(): boolean {
  return useSyncExternalStore(
    subscribeToVisibility,
    () => document.visibilityState === "visible",
    () => true,
  );
}

/** Sent by Rust when the menu-bar popover opens (`true`) or closes (`false`) (menubar.rs). */
export const POPOVER_SHOWN = "popover-shown";

/**
 * Whether Kleari is open to look at: its window is visible, or its menu-bar popover is open
 * (the main window counts as hidden then). The System and Processes readings run while it is,
 * whichever screen is showing, so both open with recent data.
 */
export function useKleariInView(): boolean {
  const visible = useDocumentVisible();
  const [popover, setPopover] = useState(false);
  useEffect(() => {
    if (!isTauri()) {
      return;
    }
    let stop: (() => void) | undefined;
    let closed = false;
    listen<unknown>(POPOVER_SHOWN, (event) => setPopover(event.payload === true)).then(
      (unlisten) => {
        if (closed) {
          unlisten();
        } else {
          stop = unlisten;
        }
      },
      // Not listening only means readings follow the window alone, as they did before.
      () => undefined,
    );
    return () => {
      closed = true;
      stop?.();
    };
  }, []);
  return visible || popover;
}

export function useSystemMetrics(
  load: () => Promise<SystemMetrics> = fetchSystemMetrics,
  intervalMs: number = REFRESH_INTERVAL_MS,
) {
  const [state, dispatch] = useReducer(reduceMetricsState, { status: "loading" });
  const pollerRef = useRef<Poller | null>(null);
  const visible = useDocumentVisible();
  // Survives effect restarts, so a new poller joins a still-pending request.
  const sharedLoad = useMemo(() => singleFlight(load), [load]);

  useEffect(() => {
    if (!visible) {
      return;
    }
    const poller = startPolling({
      load: sharedLoad,
      intervalMs,
      onSuccess: (metrics) => dispatch({ type: "success", metrics }),
      onError: (error) => dispatch({ type: "failure", error: toErrorKind(error) }),
    });
    pollerRef.current = poller;
    return () => {
      poller.stop();
      pollerRef.current = null;
    };
  }, [visible, sharedLoad, intervalMs]);

  const refresh = useCallback(() => pollerRef.current?.refresh(), []);

  return { state, refresh };
}
