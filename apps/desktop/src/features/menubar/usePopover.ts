import { useCallback, useEffect, useRef, useState } from "react";
import { parseIssues, type Issue } from "../../contracts/issues";
import type { QuarantineList, ReviewList } from "../../contracts/cleanup";
import type { StorageOverview } from "../../contracts/storageAnalysis";
import type { SystemActivity, SystemDetails } from "../../contracts/systemPage";
import { getCleanupReview, listQuarantine } from "../../lib/cleanupIpc";
import { hidePopover, openMain, quitKleari, type MenuBarDestination } from "../../lib/menubarIpc";
import { call, fetchStorageOverview } from "../../lib/storageIpc";
import { fetchSystemActivity, fetchSystemDetails } from "../../lib/systemPageIpc";
import { startPolling } from "../overview/metricsPoller";
import { toErrorKind } from "../overview/useSystemMetrics";
import { ACTIVITY_INTERVAL_MS } from "../system/useSystemPage";
import type { Loadable } from "../system/systemView";
import { buildPopover } from "./popoverView";

export interface PopoverLoaders {
  activity: () => Promise<SystemActivity>;
  details: () => Promise<SystemDetails>;
  overview: () => Promise<StorageOverview>;
  review: () => Promise<ReviewList>;
  quarantine: () => Promise<QuarantineList>;
  issues: () => Promise<Issue[]>;
  open: (destination: MenuBarDestination) => Promise<void>;
  hide: () => Promise<void>;
  quit: () => Promise<void>;
}

const defaultLoaders: PopoverLoaders = {
  activity: fetchSystemActivity,
  details: fetchSystemDetails,
  overview: fetchStorageOverview,
  review: getCleanupReview,
  quarantine: listQuarantine,
  issues: () => call("get_issues", {}, parseIssues),
  open: openMain,
  hide: hidePopover,
  quit: quitKleari,
};

function failed<T>(current: Loadable<T>, error: unknown): Loadable<T> {
  return { status: "error", error: toErrorKind(error), last: current.status === "ready" ? current.value : current.status === "error" ? current.last : null };
}

/**
 * Which time the popover is shown: 0 while hidden, a new number each time it opens. It is
 * shown exactly while its window has focus, since it closes when it loses focus (made hidden
 * at launch, it has none until the first click).
 */
function useShown(): number {
  const opened = useRef(0);
  const [shown, setShown] = useState(() => (document.hasFocus() ? ++opened.current : 0));
  useEffect(() => {
    const onFocus = () => setShown(++opened.current);
    const onBlur = () => setShown(0);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
    };
  }, []);
  return shown;
}

/** Read once each time the popover opens; the last value stays through a failed read. */
function useReadWhenShown<T>(load: () => Promise<T>, shown: number): Loadable<T> {
  const [state, setState] = useState<Loadable<T>>({ status: "loading" });
  useEffect(() => {
    if (shown === 0) {
      return;
    }
    let cancelled = false;
    load().then(
      (value) => !cancelled && setState({ status: "ready", value }),
      (error: unknown) => !cancelled && setState((current) => failed(current, error)),
    );
    return () => {
      cancelled = true;
    };
  }, [load, shown]);
  return state;
}

/** Live readings, sampled as often as the System page's, only while the popover is open. */
function useActivityWhileShown(load: () => Promise<SystemActivity>, open: boolean): Loadable<SystemActivity> {
  const [state, setState] = useState<Loadable<SystemActivity>>({ status: "loading" });
  useEffect(() => {
    if (!open) {
      return;
    }
    const poller = startPolling({
      load,
      intervalMs: ACTIVITY_INTERVAL_MS,
      onSuccess: (value) => setState({ status: "ready", value }),
      onError: (error) => setState((current) => failed(current, error)),
    });
    return () => poller.stop();
  }, [load, open]);
  return state;
}

/**
 * The menu-bar popover's data and actions. Its window is made at launch and kept hidden
 * between clicks; nothing is read until it opens, and readings stop when it closes. Escape
 * closes it.
 */
export function usePopover(loaders: PopoverLoaders = defaultLoaders) {
  const shown = useShown();
  const activity = useActivityWhileShown(loaders.activity, shown > 0);
  const details = useReadWhenShown(loaders.details, shown);
  const overview = useReadWhenShown(loaders.overview, shown);
  const review = useReadWhenShown(loaders.review, shown);
  const quarantine = useReadWhenShown(loaders.quarantine, shown);
  const issues = useReadWhenShown(loaders.issues, shown);
  const [failedAction, setFailedAction] = useState(false);
  const { open, hide, quit } = loaders;

  const run = useCallback((action: () => Promise<void>) => {
    setFailedAction(false);
    action().catch(() => setFailedAction(true));
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        run(hide);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [hide, run]);

  const data = buildPopover({ activity, details, overview, review, quarantine, issues, nowMs: Date.now() });
  return {
    data,
    /** An action could not reach Kleari; the popover says so. */
    failed: failedAction,
    go: (destination: MenuBarDestination) => run(() => open(destination)),
    quit: () => run(quit),
  };
}
