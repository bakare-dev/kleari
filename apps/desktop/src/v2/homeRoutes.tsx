import { useIssues } from "../features/issues/useIssues";
import { IssuesCard } from "./screens/dashboard/IssuesCard";
import { useEffect } from "react";
import { activityDays, activityStats } from "../features/activity/activityView";
import type { Activity } from "../features/activity/useActivity";
import { historyReasonText } from "../features/cleanup/cleanupView";
import type { useCleanupReview } from "../features/cleanup/useCleanupReview";
import type { useQuarantine } from "../features/cleanup/useQuarantine";
import { buildDashboard } from "../features/dashboard/dashboardView";
import type { Appearance } from "../features/settings/useAppearance";
import { useStorageOverview, type useStorageAnalysis } from "../features/storage/useStorageAnalysis";
import type { SystemMonitor } from "../features/system/useSystemPage";
import { DashboardScreen } from "./screens/dashboard/DashboardScreen";
import type { Settings } from "../features/settings/useSettings";
import type { Updates } from "../features/settings/useUpdates";
import { openSystemSettings } from "../lib/systemPageIpc";
import { navigate } from "./shell/routes";
import { ActivityScreen } from "./screens/activity/ActivityScreen";
import { SettingsScreen } from "./screens/settings/SettingsScreen";
import { MouseScreen } from "./screens/mouse/MouseScreen";
import type { Mouse } from "../features/mouse/useMouse";
import type { MouseCombos } from "../features/mouse/useMouseCombos";
import { useMouseFinder } from "../features/mouse/useMouseFinder";

// Activity, Settings and Dashboard: live data from this Mac, shared by the development
// interface (V2App) and release builds (ReleaseApp). Nothing here imports sample data.

type CleanupReview = ReturnType<typeof useCleanupReview>;
type Quarantine = ReturnType<typeof useQuarantine>;

/** Read again on each visit; the last list shows meanwhile. */
export function ActivityRoute({ activity }: { activity: Activity }) {
  const { refresh, list } = activity;
  useEffect(() => {
    refresh();
  }, [refresh]);
  const nowMs = Date.now();
  const events = list?.status === "available" ? list.events : [];
  return (
    <ActivityScreen
      days={activityDays(events, nowMs)}
      stats={activityStats(events, nowMs)}
      retentionDays={list?.status === "available" ? list.retentionDays : 90}
      truncated={list?.status === "available" && list.truncated}
      unavailable={list?.status === "unavailable" ? historyReasonText[list.reason] : activity.error}
      loading={list === null && activity.error === null}
    />
  );
}

/**
 * Live readings from the app-level System monitor (sampled since launch), the disk's capacity
 * read on each visit, and this session's last analysis, scan and quarantine. Opening it never
 * starts a scan; Analyze starts the same read-only analysis as Storage.
 */
export function DashboardRoute(props: {
  monitor: SystemMonitor;
  storage: ReturnType<typeof useStorageAnalysis>;
  review: CleanupReview;
  quarantine: Quarantine;
  settings: Settings;
  onSearch: (query: string) => void;
}) {
  // Read when the Dashboard opens; nothing here changes anything.
  const issues = useIssues();
  const { monitor, storage, review, quarantine, settings } = props;
  const overview = useStorageOverview(storage.state.result);
  const { refresh: refreshReview } = review;
  const { refresh: refreshQuarantine } = quarantine;
  useEffect(() => {
    refreshReview();
    refreshQuarantine();
  }, [refreshReview, refreshQuarantine]);
  // Live readings are sampled only while a page that shows them is on screen.
  const { watch, unwatch } = monitor;
  useEffect(() => {
    watch();
    return unwatch;
  }, [watch, unwatch]);
  const data = buildDashboard({
    activity: monitor.activity,
    history: monitor.history,
    details: monitor.details,
    overview,
    analysis: storage.state,
    review: review.review,
    quarantine: quarantine.list,
    reminderGb: settings.state?.preferences.preferences.quarantineReminderGb ?? 0,
    nowMs: Date.now(),
  });
  return <DashboardScreen data={data} onSearch={props.onSearch} onAnalyze={storage.analyze} alerts={<IssuesCard issues={issues.issues} failed={issues.failed} />} />;
}

/** `mouse` is read on each visit, and while waiting for Accessibility, which is turned on in
 * System Settings; it and `combos` are kept by the app, so saves finish after leaving. */
export function MouseRoute({ settings, mouse, combos }: { settings: Settings; mouse: Mouse; combos: MouseCombos }) {
  const { finder, lit, unavailable } = useMouseFinder();
  // Show in menu bar and Open at login keep Kleari, and so these settings, running.
  const { refresh } = settings;
  useEffect(() => {
    refresh();
  }, [refresh]);
  return (
    <MouseScreen
      status={mouse.status}
      finder={finder}
      lit={lit}
      finderUnavailable={unavailable}
      error={mouse.error}
      onSave={mouse.save}
      combos={combos}
      onRequestAccess={mouse.requestAccess}
      onOpenAccessibility={() => void openSystemSettings("accessibility")}
      menuBar={settings.state?.preferences.preferences.menuBar ?? null}
      openAtLogin={settings.state?.loginItem ?? null}
    />
  );
}

/** Read again on each visit (permissions can change in System Settings meanwhile). */
export function SettingsRoute({ settings, appearance, activity, updates }: { settings: Settings; appearance: Appearance; activity: Activity; updates: Updates }) {
  const { refresh } = settings;
  useEffect(() => {
    refresh();
  }, [refresh]);
  return (
    <SettingsScreen
      appearance={appearance.preference}
      appearanceStatus={appearance.storageStatus}
      windowThemeFailed={appearance.windowThemeFailed}
      onAppearanceChange={appearance.choose}
      state={settings.state}
      error={settings.error}
      saveFailed={settings.saveFailed}
      onSave={settings.save}
      onOpenAtLogin={settings.openAtLogin}
      onNotify={settings.notify}
      onSchedule={settings.schedule}
      onAddExclusion={settings.addExclusion}
      onRemoveExclusion={settings.removeExclusion}
      onRemoveFolder={settings.removeFolder}
      onRevealQuarantine={settings.revealQuarantine}
      onClearLog={activity.clear}
      updates={updates.state}
      onCheckUpdates={() => void updates.check()}
      onInstallUpdate={() => void updates.install()}
      onUninstall={settings.uninstall}
      onQuit={settings.quit}
      onOpenQuarantine={() => navigate("quarantine")}
      onOpenCleanup={() => navigate("cleanup")}
      onOpenPane={(pane) => void openSystemSettings(pane)}
    />
  );
}
