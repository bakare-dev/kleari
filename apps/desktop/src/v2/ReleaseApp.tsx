import { useCallback, useState, type ReactNode } from "react";
import { useSettingsShortcut } from "../app/navigation";
import { useActivity } from "../features/activity/useActivity";
import { useApps } from "../features/apps/useApps";
import { useFiles } from "../features/files/useFiles";
import { useMaintenance } from "../features/maintenance/useMaintenance";
import { MaintenanceScreen } from "./screens/maintenance/MaintenanceScreen";
import { BrowserScreen } from "./screens/browser/BrowserScreen";
import { openSystemSettings } from "../lib/systemPageIpc";
import { FilesScreen } from "./screens/files/FilesScreen";
import { useStorageExplorer } from "../features/storage/useStorageExplorer";
import { scanOptionsFromInput } from "../features/cleanup/cleanupView";
import { useCleanupReview } from "../features/cleanup/useCleanupReview";
import { useCleanupScan } from "../features/cleanup/useCleanupScan";
import { useScheduledScanFinished } from "../features/cleanup/useScheduledScan";
import { useQuarantine } from "../features/cleanup/useQuarantine";
import { useDeveloperScan } from "../features/developer/useDeveloperScan";
import { useMouse } from "../features/mouse/useMouse";
import { useMouseCombos } from "../features/mouse/useMouseCombos";
import { useAppearance } from "../features/settings/useAppearance";
import { useSettings } from "../features/settings/useSettings";
import { automaticUpdates, useUpdates } from "../features/settings/useUpdates";
import { UpdateOffer } from "./screens/settings/Updates";
import { useStorageAnalysis } from "../features/storage/useStorageAnalysis";
import { useSystemMonitor } from "../features/system/useSystemPage";
import { useProcessMonitor } from "../features/processes/useProcessesPage";
import type { Preferences } from "../contracts/settings";
import { Sidebar } from "./shell/Sidebar";
import { mainNav, navigate, useFocusHeadingOnRouteChange, useV2Route, v2Routes, type V2Route } from "./shell/routes";
import { ActivityRoute, DashboardRoute, MouseRoute, SettingsRoute } from "./homeRoutes";
import { AppsRoute, CleanupRoute, DeveloperRoute, PreviewRoute, ProcessesRoute, QuarantineRoute, ReviewRoute, StorageRoute, SystemRoute } from "./liveRoutes";
import "./v2.css";

/** The destinations release builds ship: every screen, each backed by real data. */
export const releaseRoutes = v2Routes;

interface ReleaseAppProps {
  /** Development builds add the development-build badge here; release builds add nothing. */
  footer?: (route: V2Route) => ReactNode;
}

/**
 * The interface: the v2 shell with every screen on live data from this Mac. Release builds
 * render it as is; development builds (V2App) add only a badge saying so. Nothing here
 * imports src/v2/sample/; scripts/check-release-bundle.mjs proves it after build.
 */
export function ReleaseApp({ footer }: ReleaseAppProps) {
  const route = useV2Route(releaseRoutes, "dashboard");
  const appearance = useAppearance();
  const [reviewQuery, setReviewQuery] = useState("");
  // App-level so running scans, the review selection, an approval or a permanent delete and
  // their results survive switching destinations; System is sampled from launch.
  const storage = useStorageAnalysis();
  const developer = useDeveloperScan();
  const review = useCleanupReview();
  const cleanup = useCleanupScan(review.refresh);
  const quarantine = useQuarantine();
  const system = useSystemMonitor();
  // Like System: read from launch while Kleari is open to look at, faster on its page.
  const processes = useProcessMonitor();
  const activity = useActivity();
  // App-level so a running Apps scan and its result survive leaving the page.
  const apps = useApps(review.refresh);
  // App-level so the folder you were browsing is still open when you come back to Storage.
  const explorer = useStorageExplorer(review.refresh);
  // App-level so a running search and each tool's result survive leaving the page.
  const files = useFiles(review.refresh);
  // App-level so a running task's result still shows after leaving the page.
  const maintenance = useMaintenance(review.refresh);
  // App-level so a Mouse save, and the combination changes waiting their turn, finish after
  // leaving the Mouse page; read and watched only while it is open.
  const mouse = useMouse(route === "mouse");
  const mouseCombos = useMouseCombos(mouse.status?.settings.combos ?? null, mouse.save);
  // A scheduled scan replaced the review (and was logged): show both as they are now.
  const { refresh: refreshReview } = review;
  const { refresh: refreshActivity } = activity;
  useScheduledScanFinished(
    useCallback(() => {
      refreshReview();
      refreshActivity();
    }, [refreshReview, refreshActivity]),
  );
  // Cleanup starts from the defaults saved in Settings, at launch and whenever they change.
  const { applyDefaults } = cleanup;
  const onPreferences = useCallback(
    (preferences: Preferences) => applyDefaults(preferences.defaultScan, scanOptionsFromInput(preferences.scanDefaults)),
    [applyDefaults],
  );
  const settings = useSettings(onPreferences);
  // Checks at launch once the saved choice is known; only an update found is shown.
  const updates = useUpdates(automaticUpdates(settings.state));
  useSettingsShortcut();
  useFocusHeadingOnRouteChange(route);

  const screens: Record<V2Route, () => ReactNode> = {
    dashboard: () => (
      <DashboardRoute
        monitor={system}
        storage={storage}
        review={review}
        quarantine={quarantine}
        settings={settings}
        onSearch={(query) => {
          setReviewQuery(query);
          navigate("review");
        }}
      />
    ),
    cleanup: () => <CleanupRoute cleanup={cleanup} review={review} />,
    review: () => <ReviewRoute review={review} initialQuery={reviewQuery} />,
    preview: () => <PreviewRoute review={review} quarantine={quarantine} />,
    quarantine: () => <QuarantineRoute quarantine={quarantine} />,
    storage: () => <StorageRoute storage={storage} review={review} cleanup={cleanup} explorer={explorer} />,
    system: () => <SystemRoute monitor={system} />,
    developer: () => <DeveloperRoute developer={developer} review={review} />,
    apps: () => <AppsRoute apps={apps} />,
    files: () => <FilesScreen files={files} />,
    browser: () => <BrowserScreen monitor={processes} />,
    maintenance: () => <MaintenanceScreen maintenance={maintenance} onOpenLoginItems={() => void openSystemSettings("loginItems")} />,
    processes: () => <ProcessesRoute monitor={processes} />,
    mouse: () => <MouseRoute settings={settings} mouse={mouse} combos={mouseCombos} />,
    activity: () => <ActivityRoute activity={activity} />,
    settings: () => <SettingsRoute settings={settings} appearance={appearance} activity={activity} updates={updates} />,
  };

  return (
    <div className="v2-shell">
      <div className="v2-titlebar" data-tauri-drag-region aria-hidden="true" />
      <Sidebar route={route} destinations={mainNav} footer={footer?.(route)} />
      <main className="v2-main" key={route}>
        {screens[route]()}
      </main>
      <UpdateOffer offer={updates.offer} state={updates.state} onInstall={() => void updates.install()} onLater={updates.dismiss} />
    </div>
  );
}
