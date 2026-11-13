import { useCallback, useEffect, useMemo, useState } from "react";
import type { StartupItems, SystemActivity, SystemDetails } from "../../contracts/systemPage";
import { fetchStartupItems, fetchSystemActivity, fetchSystemDetails, openSystemSettings, type SettingsPane } from "../../lib/systemPageIpc";
import { singleFlight, startPolling } from "../overview/metricsPoller";
import { toErrorKind, useKleariInView } from "../overview/useSystemMetrics";
import { appendHistory, buildSystemView, type Loadable } from "./systemView";

/** While the System page is on screen. */
export const ACTIVITY_INTERVAL_MS = 1_000;
/** While it isn't, so the page has recent readings and charts the moment it opens. */
export const BACKGROUND_INTERVAL_MS = 5_000;

export interface SystemLoaders {
  activity: () => Promise<SystemActivity>;
  details: () => Promise<SystemDetails>;
  startup: () => Promise<StartupItems>;
  openSettings: (pane: SettingsPane) => Promise<boolean>;
}

const defaultLoaders: SystemLoaders = {
  activity: fetchSystemActivity,
  details: fetchSystemDetails,
  startup: fetchStartupItems,
  openSettings: openSystemSettings,
};

interface LiveState {
  activity: Loadable<SystemActivity>;
  history: SystemActivity[];
}

function failed<T>(state: Loadable<T>, error: unknown): Loadable<T> {
  const last = state.status === "ready" ? state.value : state.status === "error" ? state.last : null;
  return { status: "error", error: toErrorKind(error), last };
}

/**
 * Loads at mount and again on each `reload`; a reload keeps showing the last value until
 * the new one arrives. A failure is kept as state, never swallowed.
 */
function useReloadable<T>(load: () => Promise<T>): [Loadable<T>, () => void] {
  const [state, setState] = useState<Loadable<T>>({ status: "loading" });
  const [round, setRound] = useState(0);
  useEffect(() => {
    let cancelled = false;
    load().then(
      (value) => !cancelled && setState({ status: "ready", value }),
      (error: unknown) => !cancelled && setState((current) => failed(current, error)),
    );
    return () => {
      cancelled = true;
    };
  }, [load, round]);
  const reload = useCallback(() => setRound((current) => current + 1), []);
  return [state, reload];
}

/**
 * The System page's data, held at the app level from launch. Activity is sampled every
 * `foregroundMs` while the page is on screen and every `backgroundMs` otherwise, while Kleari
 * is open to look at (its window, or its menu-bar popover) and not paused, so the page opens
 * with recent readings and charts.
 * Details and startup items are read at launch and again each time the page opens. History
 * is only what was actually sampled; a gap (pause, hidden window) starts a new chart.
 */
export function useSystemMonitor(loaders: SystemLoaders = defaultLoaders, foregroundMs = ACTIVITY_INTERVAL_MS, backgroundMs = BACKGROUND_INTERVAL_MS) {
  const [paused, setPaused] = useState(false);
  const [onScreen, setOnScreen] = useState(false);
  const [live, setLive] = useState<LiveState>({ activity: { status: "loading" }, history: [] });
  const visible = useKleariInView();
  const sharedActivity = useMemo(() => singleFlight(loaders.activity), [loaders.activity]);
  const [details, reloadDetails] = useReloadable(loaders.details);
  const [startup, reloadStartup] = useReloadable(loaders.startup);
  const intervalMs = onScreen ? foregroundMs : backgroundMs;
  // Background samples are further apart; anything closer than three of them is one chart.
  const maxGapMs = Math.max(foregroundMs, backgroundMs) * 3;

  useEffect(() => {
    if (!visible || paused) {
      return;
    }
    const poller = startPolling({
      load: sharedActivity,
      intervalMs,
      onSuccess: (sample) =>
        setLive((current) => ({
          activity: { status: "ready", value: sample },
          history: appendHistory(current.history, sample, maxGapMs),
        })),
      onError: (error) => setLive((current) => ({ ...current, activity: failed(current.activity, error) })),
    });
    return () => poller.stop();
  }, [visible, paused, sharedActivity, intervalMs, maxGapMs]);

  /** Called by the page while it is on screen; reads details and startup items again. */
  const enter = useCallback(() => {
    setOnScreen(true);
    reloadDetails();
    reloadStartup();
  }, [reloadDetails, reloadStartup]);
  const leave = useCallback(() => setOnScreen(false), []);
  const togglePause = useCallback(() => setPaused((current) => !current), []);

  return { activity: live.activity, history: live.history, details, startup, paused, togglePause, enter, leave, openSettings: loaders.openSettings };
}

export type SystemMonitor = ReturnType<typeof useSystemMonitor>;

/** The System page's view, from the app-level monitor; marks the page as on screen while mounted. */
export function useSystemPage(monitor: SystemMonitor) {
  const { enter, leave } = monitor;
  useEffect(() => {
    enter();
    return leave;
  }, [enter, leave]);
  const view = buildSystemView({
    activity: monitor.activity,
    history: monitor.history,
    details: monitor.details,
    startup: monitor.startup,
    paused: monitor.paused,
    nowMs: monitor.activity.status === "ready" ? monitor.activity.value.sampledAtMs : Date.now(),
  });
  return { view, paused: monitor.paused, togglePause: monitor.togglePause, openSettings: monitor.openSettings };
}
