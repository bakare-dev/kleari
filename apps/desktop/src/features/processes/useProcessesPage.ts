import { useCallback, useEffect, useMemo, useState } from "react";
import type { ProcessList } from "../../contracts/processes";
import { fetchProcessList, quitApp, type QuitResult } from "../../lib/processesIpc";
import { singleFlight, startPolling } from "../overview/metricsPoller";
import { toErrorKind, useDocumentVisible } from "../overview/useSystemMetrics";
import type { Loadable } from "../system/systemView";
import type { ProcessSample } from "../../v2/screens/processes/ProcessesScreen";
import { appendCpuHistory, buildProcessesView, emptyHistory, PROCESS_INTERVAL_MS, type CpuHistory } from "./processesView";

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
 * The Processes page's data, held at the app level so its charts survive leaving the page.
 * Listing every process is Kleari's costliest reading, so the list is read every
 * `intervalMs` only while Processes or Browser care is on screen (`enter`), Kleari is open to
 * look at and the list isn't paused, single-flight; never in the background. CPU history is
 * only what was sampled; a gap starts a new chart.
 */
export function useProcessMonitor(loaders: ProcessLoaders = defaultLoaders, intervalMs = PROCESS_INTERVAL_MS) {
  const [paused, setPaused] = useState(false);
  const [onScreen, setOnScreen] = useState(false);
  const [live, setLive] = useState<LiveState>({ list: { status: "loading" }, history: emptyHistory });
  const inView = useDocumentVisible();
  const sharedList = useMemo(() => singleFlight(loaders.list), [loaders.list]);
  // Anything closer than three lists is one chart.
  const maxGapMs = intervalMs * 3;

  useEffect(() => {
    if (!inView || paused || !onScreen) {
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
  }, [inView, paused, onScreen, sharedList, intervalMs, maxGapMs]);

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
