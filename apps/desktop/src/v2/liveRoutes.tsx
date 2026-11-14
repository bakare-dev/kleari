import { useEffect } from "react";
import {
  categoryCounts,
  clock,
  consequences,
  findingView,
  historyReasonText,
  lastScanSummary,
  modeDefinitions,
  moveFailureText,
  oldestText,
  quarantineBatches,
  reviewSubtitle,
  scopeFolders,
} from "../features/cleanup/cleanupView";
import type { useCleanupReview } from "../features/cleanup/useCleanupReview";
import type { useCleanupScan } from "../features/cleanup/useCleanupScan";
import type { useQuarantine } from "../features/cleanup/useQuarantine";
import { buildDeveloperView } from "../features/developer/developerView";
import type { useDeveloperScan } from "../features/developer/useDeveloperScan";
import type { Apps } from "../features/apps/useApps";
import type { StorageExplorer as StorageExplorerState } from "../features/storage/useStorageExplorer";
import { AppsScreen } from "./screens/apps/AppsScreen";
import { useProcessesPage, type ProcessMonitor } from "../features/processes/useProcessesPage";
import { useSystemPage, type SystemMonitor } from "../features/system/useSystemPage";
import { buildStorageView } from "../features/storage/storageAnalysisView";
import { openFullDiskAccessSettings, useStorageAnalysis, useStorageOverview } from "../features/storage/useStorageAnalysis";
import { openSystemSettings } from "../lib/systemPageIpc";
import { navigate } from "./shell/routes";
import { CleanupScreen } from "./screens/cleanup/CleanupScreen";
import { PreviewScreen } from "./screens/cleanup/PreviewScreen";
import { ReviewScreen } from "./screens/cleanup/ReviewScreen";
import type { ScanMode } from "./screens/cleanup/types";
import { DeveloperScreen } from "./screens/developer/DeveloperScreen";
import { ProcessesScreen } from "./screens/processes/ProcessesScreen";
import { QuarantineScreen } from "./screens/quarantine/QuarantineScreen";
import { StorageScreen } from "./screens/storage/StorageScreen";
import { SystemScreen } from "./screens/system/SystemScreen";

// The destinations that show live data from this Mac. Shared by the development interface
// (V2App) and release builds (ReleaseApp); nothing here imports sample data.

type CleanupScan = ReturnType<typeof useCleanupScan>;
type CleanupReview = ReturnType<typeof useCleanupReview>;
type Quarantine = ReturnType<typeof useQuarantine>;

/** Readings refresh only while System is on screen. */
/** Sampled from launch by the app-level monitor, faster while this page is open. */
export function SystemRoute({ monitor }: { monitor: SystemMonitor }) {
  const system = useSystemPage(monitor);
  return <SystemScreen view={system.view} paused={system.paused} onTogglePause={system.togglePause} onOpenSettings={system.openSettings} />;
}

/** Read from launch by the app-level monitor, every 2 s while this page is open. */
export function ProcessesRoute({ monitor }: { monitor: ProcessMonitor }) {
  const processes = useProcessesPage(monitor);
  return <ProcessesScreen {...processes.view} paused={processes.paused} onTogglePause={processes.togglePause} onQuit={processes.quit} />;
}

/** The scan runs only when asked; used space (for the share) is read on each visit. */
export function DeveloperRoute({ developer, review }: { developer: ReturnType<typeof useDeveloperScan>; review: CleanupReview }) {
  const overview = useStorageOverview(developer.state.result);
  const volume = overview.status === "ready" && overview.value.volume.status === "available" ? overview.value.volume.value : null;
  const view = buildDeveloperView({ state: developer.state, usedBytes: volume?.usedBytes ?? null, addedFolders: developer.addedFolders, nowMs: Date.now() });
  return (
    <DeveloperScreen
      {...view}
      onScan={developer.scan}
      onCancel={developer.cancel}
      onAddFolder={developer.addFolder}
      sent={review.developerSent}
      onSendToReview={(entries) => review.sendDeveloper(entries.flatMap((entry) => entry.reviewIds))}
    />
  );
}

/** The Apps scan runs only when asked; removing an app always asks first. */
export function AppsRoute({ apps }: { apps: Apps }) {
  const { state } = apps;
  return (
    <AppsScreen
      result={state.result}
      running={state.running}
      note={state.error ?? (state.stopped ? "The scan was stopped. Showing the last full scan." : null)}
      sent={apps.sent}
      onScan={apps.scan}
      onCancel={apps.cancel}
      onSend={apps.send}
      onRemove={(app) => apps.remove(app.id, app.name)}
      onOpenAppManagement={() => {
        void openSystemSettings("appManagement");
      }}
    />
  );
}

/** Capacity is read on each visit; the analysis runs only when asked. */
export function StorageRoute({ storage, review, cleanup, explorer }: { storage: ReturnType<typeof useStorageAnalysis>; review: CleanupReview; cleanup: CleanupScan; explorer: StorageExplorerState }) {
  const overview = useStorageOverview(storage.state.result);
  const view = buildStorageView({ overview, analysis: storage.state, nowMs: Date.now() });
  return (
    <StorageScreen
      {...view}
      onAnalyze={storage.analyze}
      onCancelAnalysis={storage.cancel}
      onOpenPrivacySettings={openFullDiskAccessSettings}
      inReview={review.storageSent}
      onAddToReview={(file) => review.addStorageFile(file.reviewId, file.name)}
      explorer={explorer}
      onScanInCleanup={(mode: ScanMode) => {
        // Opens Cleanup with the mode chosen; the scan starts only when asked there.
        cleanup.prepare({ mode });
        navigate("cleanup");
      }}
    />
  );
}

/** The folders and the review are read on each visit; a scan runs only when asked. */
export function CleanupRoute({ cleanup, review }: { cleanup: CleanupScan; review: CleanupReview }) {
  const { refreshScope } = cleanup;
  const { refresh } = review;
  useEffect(() => {
    refreshScope();
    refresh();
  }, [refreshScope, refresh]);
  const nowMs = Date.now();
  const folders = scopeFolders(cleanup.scope ?? []);
  const options = cleanup.scan.phase === "idle" ? cleanup.options : cleanup.scan.options;
  return (
    <CleanupScreen
      modes={modeDefinitions({ options, folders, result: cleanup.scan.result, review: review.review })}
      folders={folders}
      lastScan={lastScanSummary(review.review, nowMs)}
      scan={cleanup.scan}
      notices={{
        fullDiskAccess: cleanup.scan.result?.needsFullDiskAccess ?? false,
        skipped: cleanup.scan.result?.skippedFolders ?? [],
        error: cleanup.scan.error ?? cleanup.scopeError,
      }}
      preset={cleanup.preset}
      options={cleanup.options}
      onOptionsChange={cleanup.setOptions}
      onStart={cleanup.start}
      onCancel={cleanup.cancel}
      onReset={cleanup.reset}
      onAddFolder={cleanup.addFolder}
      onRequestAccess={cleanup.requestAccess}
      onOpenFilesAndFolders={() => void openSystemSettings("filesAndFolders")}
      onOpenFullDiskAccess={() => void openFullDiskAccessSettings()}
    />
  );
}

/** Step 2: the review is read again on each visit. */
export function ReviewRoute({ review, initialQuery = "" }: { review: CleanupReview; initialQuery?: string }) {
  const { refresh } = review;
  useEffect(() => {
    refresh();
  }, [refresh]);
  const nowMs = Date.now();
  const findings = (review.review?.findings ?? []).map((finding) => findingView(finding, nowMs));
  return (
    <ReviewScreen
      findings={findings}
      categoryCounts={categoryCounts(findings)}
      subtitle={reviewSubtitle(review.review, nowMs)}
      selection={review.selection}
      onSelectionChange={review.setSelection}
      initialQuery={initialQuery}
      onRevealInFinder={(finding) => review.reveal(finding.id)}
      error={review.reviewError}
    />
  );
}

/** Step 3: arriving makes a preview of the selection; approval is a separate action. */
export function PreviewRoute({ review, quarantine }: { review: CleanupReview; quarantine: Quarantine }) {
  const { preview, createPreview } = review;
  const selected = (review.review?.findings ?? []).filter((finding) => review.selection.has(finding.id) && finding.eligibility === "eligible");
  const selectedKey = selected.map((finding) => finding.id).join(",");
  const previewKey = preview.items.map((finding) => finding.id).join(",");
  const { refresh: refreshQuarantine } = quarantine;
  useEffect(() => {
    refreshQuarantine();
  }, [refreshQuarantine]);
  useEffect(() => {
    // A new selection gets a new preview; a finished or running one is kept.
    // `selected` is derived from the review each render; its IDs (`selectedKey`) are what
    // decide whether a new preview is needed.
    if (selected.length > 0 && (preview.status === "idle" || (preview.status === "review" && selectedKey !== previewKey))) {
      createPreview(selected);
    }
  }, [selectedKey, preview.status, previewKey, createPreview]);

  const nowMs = Date.now();
  const items = preview.items.map((finding) => findingView(finding, nowMs));
  const stage = preview.status === "idle" ? "review" : preview.status;
  const failedId = Object.keys(preview.failures)[0];
  const failure = failedId === undefined ? undefined : preview.failures[failedId];
  const scanAt = review.review?.scan ? clock(review.review.scan.startedAtMs) : "your review";
  return (
    <PreviewScreen
      // A rebuilt preview is a new preview, so it asks for a fresh acknowledgement.
      key={preview.preview?.id ?? "none"}
      items={items}
      stage={stage}
      states={{ ...preview.states }}
      preparedAt={preview.preview ? clock(preview.preview.preparedAtMs, true) : "—"}
      scanAt={scanAt}
      finishedAt={preview.finishedAtMs === null ? "—" : clock(preview.finishedAtMs)}
      quarantineFolder={quarantine.list?.folder ?? "Kleari’s quarantine folder"}
      staleItem={items.find((item) => preview.changed.includes(item.id))}
      failedItem={items.find((item) => item.id === failedId)}
      failedText={failure === undefined ? undefined : moveFailureText[failure]}
      errorText={preview.error}
      consequences={consequences([...new Set(items.map((item) => item.category))])}
      mode={preview.mode}
      deleteError={preview.deleteError}
      onApprove={review.approve}
      onRebuild={review.rebuild}
    />
  );
}

/** Read on each visit and after every restore or delete. */
export function QuarantineRoute({ quarantine }: { quarantine: Quarantine }) {
  const { refresh } = quarantine;
  useEffect(() => {
    refresh();
  }, [refresh]);
  const nowMs = Date.now();
  const list = quarantine.list;
  const batches = list?.status === "available" ? list.batches : [];
  return (
    <QuarantineScreen
      batches={quarantineBatches(batches, nowMs)}
      restored={quarantine.restored}
      deleted={new Set()}
      job={quarantine.job}
      oldest={oldestText(batches, nowMs)}
      unavailable={
        list?.status === "unavailable"
          ? { text: historyReasonText[list.reason], folder: list.folder }
          : quarantine.error
            ? { text: quarantine.error, folder: "Kleari’s quarantine folder" }
            : null
      }
      onRestore={quarantine.restore}
      onDelete={quarantine.startDelete}
      onStopDelete={quarantine.stopAfterCurrent}
      onCloseJob={quarantine.closeJob}
      onRevealInFinder={quarantine.reveal}
      onChooseFolder={quarantine.chooseFolder}
    />
  );
}
