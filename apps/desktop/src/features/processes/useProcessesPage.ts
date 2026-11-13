import { useCallback, useEffect, useMemo, useState } from "react";
import type { ProcessList } from "../../contracts/processes";
import { fetchProcessList, quitApp, type QuitResult } from "../../lib/processesIpc";
import { singleFlight, startPolling } from "../overview/metricsPoller";
import { toErrorKind, useKleariInView } from "../overview/useSystemMetrics";
import type { Loadable } from "../system/systemView";
import type { ProcessSample } from "../../v2/screens/processes/ProcessesScreen";
import { appendCpuHistory, buildProcessesView, emptyHistory, PROCESS_INTERVAL_MS, type CpuHistory } from "./processesView";

/** While the Processes page isn't on screen, so it opens with a recent list. */
export const PROCESS_BACKGROUND_INTERVAL_MS = 5_000;

export interface ProcessLoaders {
  list: () => Promise<ProcessList>;
  quit: (pid: number, quitToken: number) => Promise<QuitResult>;
}

const defaultLoaders: ProcessLoaders = { list: fetchProcessList, quit: quitApp };

interface LiveState {
  list: Loadable<ProcessList>;
  history: CpuHistory;
}

/**
 * The PID and identity token to ask about, taken from the latest successful list (never from
 * a stale one). The backend re-checks the PID against this token, so a reused PID is never
 * asked. `null` when the row isn't a quittable app in that list.
 */
export function quitTarget(state: Loadable<ProcessList>, id: string): { pid: number; quitToken: number } | null {
  const row = state.status === "ready" ? state.value.rows.find((entry) => entry.id === id) : undefined;
  if (!row?.canQuit || row.quitToken === null) {
    return null;
  }
  return { pid: row.pid, quitToken: row.quitToken };
}

/**
 * The Processes page's data, held at the app level from launch: the list is read every
 * `foregroundMs` while the page is on screen and every `backgroundMs` otherwise, while Kleari
 * is open to look at (its window, or its menu-bar popover) and not paused, single-flight. CPU
 * history is only what was sampled; a gap starts a new chart.
 */
export function useProcessMonitor(
  loaders: ProcessLoaders = defaultLoaders,
  foregroundMs = PROCESS_INTERVAL_MS,
  backgroundMs = PROCESS_BACKGROUND_INTERVAL_MS,
) {
  const [paused, setPaused] = useState(false);
  const [onScreen, setOnScreen] = useState(false);
  const [live, setLive] = useState<LiveState>({ list: { status: "loading" }, history: emptyHistory });
  const inView = useKleariInView();
  const sharedList = useMemo(() => singleFlight(loaders.list), [loaders.list]);
  const intervalMs = onScreen ? foregroundMs : backgroundMs;
  // Background lists are further apart; anything closer than three of them is one chart.
  const maxGapMs = Math.max(foregroundMs, backgroundMs) * 3;

  useEffect(() => {
    if (!inView || paused) {
      return;
    }
    const poller = startPolling({
      load: sharedList,
      intervalMs,
      onSuccess: (list) =>
        setLive((current) => ({
          list: { status: "ready", value: list },
          history: appendCpuHistory(current.history, list, maxGapMs),
        })),
      onError: (error) =>
        setLive((current) => {
          const last = current.list.status === "ready" ? current.list.value : current.list.status === "error" ? current.list.last : null;
          return { ...current, list: { status: "error", error: toErrorKind(error), last } };
        }),
    });
    return () => poller.stop();
  }, [inView, paused, sharedList, intervalMs, maxGapMs]);

  const quit = useCallback(
    async (process: ProcessSample): Promise<QuitResult> => {
      const target = quitTarget(live.list, process.id);
      return target ? loaders.quit(target.pid, target.quitToken) : "notAllowed";
    },
    [live.list, loaders],
  );

  const enter = useCallback(() => setOnScreen(true), []);
  const leave = useCallback(() => setOnScreen(false), []);
  const togglePause = useCallback(() => setPaused((current) => !current), []);
  return { list: live.list, history: live.history, paused, togglePause, quit, enter, leave };
}

export type ProcessMonitor = ReturnType<typeof useProcessMonitor>;

/** The Processes page's view, from the app-level monitor; marks the page as on screen while mounted. */
export function useProcessesPage(monitor: ProcessMonitor) {
  const { enter, leave } = monitor;
  useEffect(() => {
    enter();
    return leave;
  }, [enter, leave]);
  const view = buildProcessesView({ state: monitor.list, history: monitor.history, paused: monitor.paused, nowMs: Date.now() });
  return { view, paused: monitor.paused, togglePause: monitor.togglePause, quit: monitor.quit };
}
