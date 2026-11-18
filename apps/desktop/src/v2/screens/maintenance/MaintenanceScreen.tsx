import { useEffect, useState } from "react";
import type { Task, TaskInfo, TaskOutcome } from "../../../contracts/maintenance";
import type { StartupItem } from "../../../contracts/systemPage";
import { lockDurations, type LockDuration } from "../../../contracts/keyboardLock";
import type { KeyboardLock } from "../../../features/keyboard/useKeyboardLock";
import type { Maintenance } from "../../../features/maintenance/useMaintenance";
import type { IconName } from "../../ui/icons";
import { Button, FloatingBar, IconTile, Modal, PageHeader, Pill, Segmented, Spinner, Toast, useToast, type Tone } from "../../ui/components";
import { applyFilter, FilterBar, useListFilter, type ListFilterSpec } from "../../ui/ListFilter";
import type { AppEntry } from "../../../contracts/apps";
import { formatGb, plural, sum } from "../../ui/format";
import { cacheBytes, type Apps } from "../../../features/apps/useApps";
import { headingIdFor, tabFromHash } from "../../shell/routes";
import "./maintenance.css";
import { AccessSteps } from "../../ui/AccessSteps";
import { openSystemSettings } from "../../../lib/systemPageIpc";
import { useRemoval } from "../../../features/removal/RemovalProvider";
import { Picture } from "../../ui/Picture";

const tabs = ["tasks", "caches", "login"] as const;
type Tab = (typeof tabs)[number];

export const taskMeta: Record<Task, { title: string; tone: Tone; icon: IconName; what: string; after: string }> = {
  freeRam: {
    title: "Free up memory",
    tone: "green",
    icon: "chip",
    what: "Asks macOS to purge memory it keeps for apps that aren’t using it (purge).",
    after: "Apps may feel slower for a moment while they load what they need again. macOS usually manages this well by itself.",
  },
  flushDns: {
    title: "Flush the DNS cache",
    tone: "blue",
    icon: "refresh",
    what: "Clears the addresses macOS remembers for websites.",
    after: "Fixes sites that moved or don’t load after a network change. Nothing else changes.",
  },
  thinSnapshots: {
    title: "Free up purgeable space",
    tone: "violet",
    icon: "disk",
    what: "Asks Time Machine to thin the local snapshots that hold purgeable space.",
    after: "Older local snapshots go; backups on your backup disk aren’t touched.",
  },
  reindexSpotlight: {
    title: "Reindex Spotlight",
    tone: "indigo",
    icon: "search",
    what: "Rebuilds Spotlight’s index of your startup disk.",
    after: "Search is incomplete and your Mac works harder for a while (up to an hour) as it indexes again.",
  },
  clearSystemCaches: {
    title: "Clear system caches",
    tone: "teal",
    icon: "trash",
    what: "Deletes other apps’ caches in /Library/Caches. Apple’s are never touched.",
    after: "Apps rebuild what they need. These are deleted for good, not quarantined.",
  },
  clearSystemLogs: {
    title: "Clear old system logs",
    tone: "orange",
    icon: "clock",
    what: "Deletes rotated system logs and crash reports older than 30 days.",
    after: "Recent logs stay for troubleshooting. These are deleted for good.",
  },
  removeDeletedUsers: {
    title: "Remove deleted users",
    tone: "pink",
    icon: "user",
    what: "Deletes /Users/Deleted Users, the disk images macOS kept of accounts you removed.",
    after: "Those accounts’ files can’t be recovered afterwards.",
  },
  emptyTrash: {
    title: "Empty the Trash",
    tone: "red",
    icon: "trashLines",
    what: "Asks Finder to empty the Trash on your startup disk and connected drives.",
    after: "Everything in the Trash is deleted for good. macOS asks once to let Kleari control Finder.",
  },
};

const scopeLabel: Record<StartupItem["scope"], string> = {
  userAgent: "Starts when you log in",
  allUsersAgent: "Starts for every user",
  systemDaemon: "Starts with your Mac",
};

function infoLine(info: TaskInfo | undefined): string | null {
  if (!info) {
    return null;
  }
  if (info.task === "thinSnapshots") {
    return info.count === null ? null : `${plural(info.count, "local snapshot")}`;
  }
  if (info.bytes === null) {
    return null;
  }
  const size = `${info.lowerBound ? "≥ " : ""}${formatGb(info.bytes / 1e9)}`;
  if (info.task === "emptyTrash") {
    return `${size} in the Trash${info.lowerBound ? " (Full Disk Access shows all of it)" : ""}`;
  }
  if (info.task === "removeDeletedUsers") {
    return info.count ? `${plural(info.count, "deleted account")} · ${size}` : "No deleted accounts";
  }
  return `${size} to clear`;
}

const outcomeText: Record<TaskOutcome, string> = {
  done: "Done.",
  cancelled: "Cancelled. Nothing was changed.",
  failed: "macOS didn’t finish it. Nothing more was changed.",
};

const cacheSpec: ListFilterSpec<AppEntry> = {
  bytes: cacheBytes,
  name: (app) => app.name,
  text: (app) => app.bundleId ?? app.location,
};

const lockLabels: Record<LockDuration, string> = { 30: "30 s", 60: "1 min", 120: "2 min", 300: "5 min" };

/** Keyboard cleaning mode: locks every key for the time chosen; any click unlocks. */
function KeyboardCard({ keyboard }: { keyboard: KeyboardLock }) {
  const [seconds, setSeconds] = useState<LockDuration>(60);
  return (
    <article className="v2-card mt-task">
      <div className="mt-task-head">
        <IconTile tone="blue" icon="key" size={34} />
        <h2 className="v2-card-title-sm">Clean the keyboard</h2>
      </div>
      <span className="v2-card-note">Locks every key, media keys and macOS shortcuts included, so you can wipe it. Click anywhere to unlock sooner.</span>
      <Segmented
        label="Lock for"
        value={String(seconds)}
        onChange={(value) => setSeconds(lockDurations.find((length) => String(length) === value) ?? 60)}
        options={lockDurations.map((length) => ({ value: String(length), label: lockLabels[length] }))}
      />
      {keyboard.error && <span role="status" className="v2-card-note">{keyboard.error}</span>}
      {keyboard.needsAccess && <AccessSteps kind="accessibility" compact onOpen={() => void openSystemSettings("accessibility")} />}
      <div className="mt-task-foot">
        <Button variant="soft" size="sm" disabled={keyboard.state.state === "locked"} onClick={() => keyboard.start(seconds)}>Lock keyboard</Button>
      </div>
    </article>
  );
}

export function MaintenanceScreen({ maintenance, apps, keyboard, onOpenLoginItems }: { maintenance: Maintenance; apps: Apps; keyboard: KeyboardLock; onOpenLoginItems: () => void }) {
  const { info, startup, running } = maintenance;
  const [tab, setTab] = useState<Tab>(() => tabFromHash(tabs, "tasks"));
  const [cachePick, setCachePick] = useState<ReadonlySet<string>>(new Set());
  // Installed apps with caches, from the last Apps scan, largest first.
  const cached = (apps.state.result?.apps ?? [])
    .filter((app) => !app.apple && cacheBytes(app) > 0)
    .sort((a, b) => cacheBytes(b) - cacheBytes(a));
  const [cacheFilter, setCacheFilter] = useListFilter("maintenance.caches");
  const shownCaches = applyFilter(cached, cacheFilter, cacheSpec);
  // An open app keeps its cache, so it can't be chosen.
  const choosableCaches = shownCaches.filter((app) => !app.running);
  const cachePicked = cached.filter((app) => !app.running && cachePick.has(app.id));
  const cacheBar = tab === "caches" && cachePicked.length > 0;
  const [confirm, setConfirm] = useState<Task | null>(null);
  const [acknowledged, setAcknowledged] = useState(false);
  const toast = useToast(6000);
  const removal = useRemoval();
  const { refresh } = maintenance;

  useEffect(() => {
    refresh();
  }, [refresh]);

  const run = (task: Task) => {
    setConfirm(null);
    setAcknowledged(false);
    void maintenance.run(task).then((outcome) => {
      if (outcome) {
        toast.show(`${taskMeta[task].title}: ${outcomeText[outcome]}`, outcome === "done" ? undefined : "warn");
      }
    });
  };

  const byTask = new Map((info ?? []).map((entry) => [entry.task, entry]));
  const confirming = confirm ? byTask.get(confirm) : undefined;

  return (
    <>
      <PageHeader
        title="Maintenance"
        headingId={headingIdFor("maintenance")}
        subtitle="Tasks macOS can do but keeps out of sight. Each runs only when you choose it, and macOS asks for your password when it needs one."
        actions={<Button variant="soft" size="sm" disabled={maintenance.loading} onClick={refresh}>{maintenance.loading ? <Spinner size={10} /> : null} Measure again</Button>}
      />
      <div className={`mt-body${cacheBar ? " mt-body-above-bar" : ""}`}>
        {maintenance.error && <span role="status" className="v2-card-note">{maintenance.error}</span>}
        <Segmented
          label="Section"
          value={tab}
          onChange={setTab}
          options={[
            { value: "tasks", label: "Tasks" },
            { value: "caches", label: "App caches" },
            { value: "login", label: "Login items" },
          ]}
        />
        {tab === "caches" &&
          (!apps.state.result || cached.length === 0 ? (
            <section aria-label="App caches" className="v2-card mt-empty">
              {apps.state.running ? <Spinner size={18} /> : <IconTile tone="blue" icon="apps" size={32} />}
              <span role="status" className="v2-row-title">
                {apps.state.running ? "Scanning your apps…" : apps.state.result ? "No app caches were found" : "Not scanned yet"}
              </span>
              <span className="v2-card-note">
                App caches are what apps keep in ~/Library/Caches. Apps rebuild them by themselves, so clearing them frees space without touching your settings or data.
              </span>
              {!apps.state.running && <Button variant="primary" size="sm" onClick={apps.scan}>{apps.state.result ? "Scan again" : "Scan apps"}</Button>}
            </section>
          ) : (
            <section aria-labelledby="mt-caches" className="v2-card mt-list">
              <div className="mt-list-head">
                <input
                  type="checkbox"
                  aria-label="Choose all shown"
                  checked={choosableCaches.length > 0 && choosableCaches.every((app) => cachePick.has(app.id))}
                  disabled={choosableCaches.length === 0}
                  onChange={(event) => setCachePick(event.target.checked ? new Set(choosableCaches.map((app) => app.id)) : new Set())}
                />
                <h2 id="mt-caches" className="v2-card-title-sm">App caches</h2>
                <span className="v2-muted v2-num">
                  {plural(cached.length, "app")} · {formatGb(sum(cached, (app) => cacheBytes(app)) / 1e9)}
                </span>
                <Button variant="soft" size="sm" disabled={apps.state.running} onClick={apps.scan}>
                  {apps.state.running ? <Spinner size={10} /> : null} {apps.state.running ? "Scanning…" : "Scan again"}
                </Button>
              </div>
              <span className="v2-card-note mt-list-note">Apps rebuild their caches by themselves. An app that’s open keeps its cache until you quit it.</span>
              <FilterBar label="Filter app caches" className="mt-list-note" state={cacheFilter} onChange={setCacheFilter} items={cached} spec={cacheSpec} />
              <ul className="mt-list-rows" tabIndex={0} aria-label="Apps with caches">
                {shownCaches.length === 0 && <li className="v2-muted">No apps match.</li>}
                {shownCaches.map((app) => (
                  <li key={app.id}>
                    <input
                      type="checkbox"
                      aria-label={`Choose ${app.name}`}
                      checked={!app.running && cachePick.has(app.id)}
                      disabled={app.running}
                      onChange={() => {
                        const next = new Set(cachePick);
                        if (next.has(app.id)) {
                          next.delete(app.id);
                        } else {
                          next.add(app.id);
                        }
                        setCachePick(next);
                      }}
                    />
                    <Picture source="app" id={app.id} size={28} label={app.name} fallback={<IconTile tone="blue" icon="apps" size={28} />} />
                    <span className="v2-row-text">
                      <span className="v2-row-title v2-ellipsis">{app.name}</span>
                      <span className="v2-row-sub v2-ellipsis">{app.bundleId ?? app.location}</span>
                    </span>
                    {app.running && <Pill status="warn">Open</Pill>}
                    <span className="mt-size v2-num">{formatGb(cacheBytes(app) / 1e9)}</span>
                  </li>
                ))}
              </ul>
            </section>
          ))}
        {tab === "tasks" && (
        <section aria-label="Tasks" className="mt-grid" tabIndex={0}>
          <KeyboardCard keyboard={keyboard} />
          {(Object.keys(taskMeta) as Task[]).map((task) => {
            const meta = taskMeta[task];
            const entry = byTask.get(task);
            const line = infoLine(entry);
            return (
              <article key={task} className="v2-card mt-task">
                <div className="mt-task-head">
                  <IconTile tone={meta.tone} icon={meta.icon} size={34} />
                  <h2 className="v2-card-title-sm">{meta.title}</h2>
                  {entry?.needsAdmin && <Pill status="muted">Password</Pill>}
                </div>
                <span className="v2-card-note">{meta.what}</span>
                {line && <strong className="v2-num mt-line">{line}</strong>}
                <div className="mt-task-foot">
                  {maintenance.last?.task === task && <span className="v2-card-note">{outcomeText[maintenance.last.outcome]}</span>}
                </div>
                {task === "emptyTrash" && maintenance.last?.task === "emptyTrash" && maintenance.last.outcome === "failed" && (
                  <AccessSteps kind="automation" compact onOpen={() => void openSystemSettings("automation")} />
                )}
                <div className="mt-task-foot">
                  <Button variant={entry?.deletes ? "danger" : "soft"} size="sm" disabled={running !== null} onClick={() => setConfirm(task)}>
                    {running === task ? <Spinner size={10} /> : null} {running === task ? "Running…" : "Run"}
                  </Button>
                </div>
              </article>
            );
          })}
        </section>
        )}

        {tab === "login" && (
        <section aria-labelledby="mt-login" className="v2-card mt-login">
          <div className="mt-login-head">
            <h2 id="mt-login" className="v2-card-title-sm">Login and background items</h2>
            <Button variant="soft" size="sm" onClick={onOpenLoginItems}>Open Login Items settings</Button>
          </div>
          <span className="v2-card-note">
            Programs that start by themselves. You can stop one of your own here: you'll see it first, then delete its login item or keep it in quarantine, and it stops starting from your next login. Items for every user and system items are managed in System Settings.
          </span>
          {!startup ? (
            <span className="v2-card-note">{maintenance.loading ? "Reading…" : "Not read yet."}</span>
          ) : startup.items.length === 0 ? (
            <span className="v2-card-note">No login or background items were found.</span>
          ) : (
            <ul className="mt-rows" tabIndex={0} aria-label="Login and background items">
              {startup.items.map((item) => {
                return (
                  <li key={`${item.scope}:${item.label}`}>
                    <IconTile tone={item.program === "missing" ? "red" : "indigo"} icon="power" size={28} />
                    <span className="v2-row-text">
                      <span className="v2-row-title v2-ellipsis">{item.name}</span>
                      <span className="v2-row-sub v2-ellipsis">{scopeLabel[item.scope]} · {item.label}</span>
                    </span>
                    <span className="mt-row-pill">
                      {item.program === "missing" ? <Pill status="danger">Program gone</Pill> : item.disabledInPlist ? <Pill status="muted">Off</Pill> : null}
                    </span>
                    <span className="mt-row-action">
                      {item.scope !== "userAgent" ? null : (
                        <Button
                          variant="soft"
                          size="sm"
                          onClick={() =>
                            removal({ what: item.name, prepare: () => maintenance.prepareAgent(item.label), onDone: maintenance.removed })
                          }
                        >
                          Stop at login…
                        </Button>
                      )}
                    </span>
                  </li>
                );
              })}
            </ul>
          )}
        </section>
        )}
      </div>

      {cacheBar && (
        <FloatingBar>
          <div className="v2-float-text">
            <span role="status" className="v2-float-title">
              {plural(cachePicked.length, "app")} chosen · {formatGb(sum(cachePicked, (app) => cacheBytes(app)) / 1e9)}
            </span>
            <span className="v2-float-sub">You'll see exactly what will go before anything moves.</span>
          </div>
          <div className="v2-float-actions">
            <Button variant="secondary" onClick={() => setCachePick(new Set())}>Clear</Button>
            <Button
              variant="primary"
              onClick={() =>
                removal({
                  what: `the caches of ${plural(cachePicked.length, "app")}`,
                  prepare: () => apps.prepareCaches(cachePicked.map((app) => app.id)),
                  onDone: () => {
                    setCachePick(new Set());
                    apps.refresh();
                  },
                })
              }
            >
              Clear caches…
            </Button>
          </div>
        </FloatingBar>
      )}

      <Modal open={confirm !== null} onClose={() => { setConfirm(null); setAcknowledged(false); }} labelledBy="mt-confirm-title" role="alertdialog">
        {confirm && (
          <div className="v2-dialog-body">
            <div className="v2-dialog-head">
              <IconTile tone={taskMeta[confirm].tone} icon={taskMeta[confirm].icon} size={48} glow />
              <div className="v2-row-text">
                <h2 id="mt-confirm-title" className="v2-dialog-title">{taskMeta[confirm].title}?</h2>
                {confirming?.needsAdmin && <span className="v2-card-note">macOS will ask for an administrator’s password. Kleari never sees it.</span>}
              </div>
            </div>
            <p className="v2-dialog-text">{taskMeta[confirm].what} {taskMeta[confirm].after}</p>
            {confirming?.deletes && (
              <label className={`v2-ack v2-ack-danger${acknowledged ? " on" : ""}`}>
                <input type="checkbox" checked={acknowledged} onChange={() => setAcknowledged(!acknowledged)} />
                <span>I understand this deletes them for good; they can’t be restored.</span>
              </label>
            )}
            <div className="v2-dialog-actions">
              <Button variant="secondary" onClick={() => { setConfirm(null); setAcknowledged(false); }}>Cancel</Button>
              <Button variant={confirming?.deletes ? "danger" : "primary"} disabled={Boolean(confirming?.deletes) && !acknowledged} onClick={() => run(confirm)}>
                Run
              </Button>
            </div>
          </div>
        )}
      </Modal>

      <Toast message={toast.message} bottom={cacheBar ? 92 : 26} />
    </>
  );
}
