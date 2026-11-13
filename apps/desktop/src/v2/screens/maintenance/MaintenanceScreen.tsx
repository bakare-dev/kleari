import { useEffect, useState } from "react";
import type { Task, TaskInfo, TaskOutcome } from "../../../contracts/maintenance";
import type { StartupItem } from "../../../contracts/systemPage";
import type { Maintenance } from "../../../features/maintenance/useMaintenance";
import type { IconName } from "../../ui/icons";
import { Button, IconTile, Modal, PageHeader, Pill, Spinner, Toast, useToast, type Tone } from "../../ui/components";
import { formatGb, plural } from "../../ui/format";
import { headingIdFor, hrefFor } from "../../shell/routes";
import "./maintenance.css";
import { AccessSteps } from "../../ui/AccessSteps";
import { openSystemSettings } from "../../../lib/systemPageIpc";

const taskMeta: Record<Task, { title: string; tone: Tone; icon: IconName; what: string; after: string }> = {
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

const notOffered: readonly { title: string; why: string }[] = [
  { title: "Repair disk permissions", why: "macOS protects its own permissions since El Capitan; use Disk Utility’s First Aid for disk problems." },
  { title: "Run periodic scripts", why: "Modern macOS runs its own maintenance on schedule; there is nothing left to run by hand." },
  { title: "Language files and universal binaries", why: "Removing them breaks apps’ signatures, so apps may refuse to open or update." },
  { title: "Document versions", why: "macOS keeps them in a protected folder that no app may change." },
  { title: "Mail’s Trash", why: "Mail keeps it in its own protected data; empty it in Mail (Mailbox › Erase Deleted Items)." },
];

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

export function MaintenanceScreen({ maintenance, onOpenLoginItems }: { maintenance: Maintenance; onOpenLoginItems: () => void }) {
  const { info, startup, running } = maintenance;
  const [confirm, setConfirm] = useState<Task | null>(null);
  const [acknowledged, setAcknowledged] = useState(false);
  const toast = useToast(6000);
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
      <div className="mt-body">
        {maintenance.error && <span role="status" className="v2-card-note">{maintenance.error}</span>}
        <section aria-label="Tasks" className="mt-grid">
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
                    {running === task ? <Spinner size={10} /> : null} {running === task ? "Running…" : "Run…"}
                  </Button>
                </div>
              </article>
            );
          })}
        </section>

        <section aria-labelledby="mt-login" className="v2-card mt-login">
          <div className="mt-login-head">
            <h2 id="mt-login" className="v2-card-title-sm">Login and background items</h2>
            <Button variant="soft" size="sm" onClick={onOpenLoginItems}>Open Login Items settings</Button>
          </div>
          <span className="v2-card-note">
            Programs that start by themselves. You can stop one of your own here: it goes to Cleanup’s review and, once approved, into quarantine. Items for every user and system items are managed in System Settings.
          </span>
          {!startup ? (
            <span className="v2-card-note">{maintenance.loading ? "Reading…" : "Not read yet."}</span>
          ) : startup.items.length === 0 ? (
            <span className="v2-card-note">No login or background items were found.</span>
          ) : (
            <ul className="mt-rows">
              {startup.items.map((item) => {
                const sent = maintenance.sentAgents.has(item.label);
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
                      {item.scope !== "userAgent" ? null : sent ? (
                        <Pill status="link">In review</Pill>
                      ) : (
                        <Button
                          variant="soft"
                          size="sm"
                          onClick={() => {
                            void maintenance.sendAgent(item.label).then((message) => toast.show(message));
                          }}
                        >
                          Stop at login
                        </Button>
                      )}
                    </span>
                  </li>
                );
              })}
            </ul>
          )}
        </section>

        <section aria-labelledby="mt-not" className="v2-card mt-not">
          <h2 id="mt-not" className="v2-card-title-sm">Not offered, and why</h2>
          <ul className="v2-info-list">
            {notOffered.map((item) => (
              <li key={item.title}>
                <span className="v2-row-text">
                  <span className="v2-row-title">{item.title}</span>
                  <span className="v2-row-sub">{item.why}</span>
                </span>
              </li>
            ))}
          </ul>
        </section>
      </div>

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

      <Toast message={toast.message}>
        <a className="v2-link" href={hrefFor("review")}>Open review</a>
      </Toast>
    </>
  );
}
