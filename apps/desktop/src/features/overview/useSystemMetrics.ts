import { useCallback, useEffect, useMemo, useReducer, useRef, useSyncExternalStore } from "react";
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
