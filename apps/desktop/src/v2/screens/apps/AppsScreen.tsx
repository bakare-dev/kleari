import { useState } from "react";
import type { AppEntry, AppsScan, InstallerEntry, LeftoverEntry } from "../../../contracts/apps";
import type { CleanupPreview } from "../../../contracts/cleanup";
import { Button, FloatingBar, IconTile, Modal, PageHeader, Pill, Segmented, Spinner, StatCard, Toast, useToast } from "../../ui/components";
import { formatGb, plural, sum } from "../../ui/format";
import { headingIdFor } from "../../shell/routes";
import { useRemoval } from "../../../features/removal/RemovalProvider";
import { cacheBytes } from "../../../features/apps/useApps";
import { Picture } from "../../ui/Picture";
import { applyFilter, FilterBar, useListFilter, type ListFilterSpec, type ListId, type TypeChip } from "../../ui/ListFilter";
import "../developer/developer.css";
import "./apps.css";
import { AccessSteps } from "../../ui/AccessSteps";

type View = "apps" | "leftovers" | "installers";

interface AppsScreenProps {
  result: AppsScan | null;
  running: boolean;
  /** A message about the scan: stopped, or why it failed. */
  note: string | null;
  onScan: () => void;
  onCancel: () => void;
  /** Prepares leftovers or installers for removal here (the confirm sheet shows it). */
  onPrepare: (ids: readonly string[]) => Promise<CleanupPreview>;
  /** Prepares installed apps' caches for removal here (only their folders in ~/Library/Caches). */
  onPrepareCaches: (appIds: readonly string[]) => Promise<CleanupPreview>;
  /** After something moved: scans again. */
  onRemoved: () => void;
  /** Moves the app to the Trash. Resolves to a message, and what it left behind, if anything. */
  onRemove: (app: AppEntry) => Promise<{ text: string; ok: boolean; leftovers: CleanupPreview | null }>;
  onOpenAppManagement: () => void;
}

const gb = (bytes: number) => bytes / 1e9;

const appStatus = (app: AppEntry) => (app.apple ? "macos" : app.running ? "open" : "removable");
const appChips: readonly TypeChip[] = [
  { value: "removable", label: "Can remove", tone: "green" },
  { value: "open", label: "Open", tone: "orange" },
  { value: "macos", label: "Part of macOS", tone: "gray" },
];
const appSpec: ListFilterSpec<AppEntry> = {
  bytes: (app) => app.bytes + app.leftoverBytes,
  name: (app) => app.name,
  text: (app) => `${app.location} ${app.bundleId ?? ""}`,
  date: (app) => app.modifiedMs,
  type: appStatus,
};
const itemSpec: ListFilterSpec<LeftoverEntry | InstallerEntry> = {
  bytes: (item) => item.bytes,
  name: (item) => item.name,
  text: (item) => ("owner" in item ? `${item.owner} ${item.location}` : item.location),
  date: (item) => item.modifiedMs,
};
const listIds: Record<View, ListId> = { apps: "apps", leftovers: "apps.leftovers", installers: "apps.installers" };

function size(item: { bytes: number; lowerBound?: boolean }): string {
  return `${item.lowerBound ? "≥ " : ""}${formatGb(gb(item.bytes))}`;
}

export function AppsScreen(props: AppsScreenProps) {
  const { result, running } = props;
  const removal = useRemoval();
  const [view, setView] = useState<View>("apps");
  const [focus, setFocus] = useState<string | null>(null);
  const [selection, setSelection] = useState<ReadonlySet<string>>(new Set());
  const [confirm, setConfirm] = useState<AppEntry | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const toast = useToast(6000);
  const [filter, setFilter] = useListFilter(listIds[view]);

  const apps = result?.apps ?? [];
  const orphans = result?.orphans ?? [];
  const installers = result?.installers ?? [];
  const removable = apps.filter((app) => !app.apple);
  const focused = apps.find((app) => app.id === focus) ?? removable[0] ?? null;
  const sendable = (id: string | null): id is string => id !== null;
  const items: readonly (LeftoverEntry | InstallerEntry)[] = view === "installers" ? installers : orphans;
  const shownApps = applyFilter(apps, filter, appSpec);
  const shownItems = applyFilter(items, filter, itemSpec);
  const picked = items.filter((item) => sendable(item.reviewId) && selection.has(item.reviewId ?? ""));
  const pickedBytes = sum(picked, (item) => item.bytes);

  const toggle = (id: string | null) => {
    if (!sendable(id)) {
      return;
    }
    const next = new Set(selection);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    setSelection(next);
  };

  const removePicked = () => {
    const ids = picked.map((item) => item.reviewId ?? "");
    removal({
      what: view === "installers" ? plural(ids.length, "installer") : plural(ids.length, "leftover"),
      prepare: () => props.onPrepare(ids),
      onDone: () => {
        setSelection(new Set());
        props.onRemoved();
      },
    });
  };

  const remove = (app: AppEntry) => {
    setBusy(true);
    void props.onRemove(app).then((outcome) => {
      setBusy(false);
      setConfirm(null);
      setFailed(!outcome.ok);
      toast.show(outcome.text, outcome.ok ? undefined : "warn");
      // What the app left in your Library: offered straight away, on this page.
      const leftovers = outcome.leftovers;
      if (leftovers !== null) {
        removal({ what: `what ${app.name} left behind`, prepare: () => Promise.resolve(leftovers), onDone: props.onRemoved });
      }
    });
  };

  return (
    <>
      <PageHeader
        title="Apps"
        headingId={headingIdFor("apps")}
        subtitle="Installed apps with what they keep in your Library, data left by apps that are gone, and installers you’ve already used."
        actions={
          <>
            {running ? (
              <Button variant="secondary" size="sm" onClick={props.onCancel}>
                <Spinner size={10} /> Cancel
              </Button>
            ) : (
              <Button variant="soft" size="sm" onClick={props.onScan}>{result ? "Scan again" : "Scan"}</Button>
            )}
          </>
        }
      />
      <div className={`dev-body${result && view !== "apps" ? " dev-body-above-bar" : ""}`}>
        {props.note && <span role="status" className="v2-card-note dev-status">{props.note}</span>}
        <section aria-label="Apps summary" className="v2-grid-4">
          <StatCard tone="blue" icon="apps" label="Apps" value={result ? String(removable.length) : "—"} sub={result ? `${formatGb(gb(sum(removable, (app) => app.bytes)))} installed` : "Not scanned yet"} />
          <StatCard tone="violet" icon="folder" label="Kept in your Library" value={result ? formatGb(gb(sum(removable, (app) => app.leftoverBytes))) : "—"} sub="Settings, caches and data" />
          <StatCard tone="pink" icon="archive" label="Left by removed apps" value={result ? formatGb(gb(sum(orphans, (left) => left.bytes))) : "—"} sub={result ? plural(orphans.length, "item") : "Not scanned yet"} />
          <StatCard tone="orange" icon="package" label="Installers" value={result ? formatGb(gb(sum(installers, (file) => file.bytes))) : "—"} sub={result ? plural(installers.length, "file") : "Not scanned yet"} />
        </section>

        {!result ? (
          <section aria-label={running ? "Scanning" : "Not scanned yet"} className="v2-card dev-empty">
            {running ? <Spinner size={34} /> : <IconTile tone="blue" icon="apps" size={42} glow />}
            <span className="v2-row-title">{running ? "Scanning…" : "Not scanned yet"}</span>
            <span className="v2-card-note">
              Scanning reads your Applications folders and the folders in your Library where apps keep their data. It only reads names and sizes, and each app’s Info.plist.
            </span>
            {!running && <Button variant="primary" size="sm" onClick={props.onScan}>Scan</Button>}
          </section>
        ) : (
          <div className="dev-grid">
            <section aria-labelledby="apps-list" className="v2-card dev-list">
              <div className="dev-list-head">
                <h2 id="apps-list" className="v2-card-title-sm">{view === "apps" ? "Installed apps" : view === "leftovers" ? "Left by apps that are gone" : "Installers"}</h2>
                <Segmented
                  label="View"
                  value={view}
                  onChange={(next) => {
                    setView(next);
                    setSelection(new Set());
                  }}
                  options={[
                    { value: "apps", label: "Apps" },
                    { value: "leftovers", label: "Leftovers" },
                    { value: "installers", label: "Installers" },
                  ]}
                />
              </div>
              <span className="v2-card-note dev-list-note">
                {view === "apps"
                  ? "Removing an app moves it to the Trash, then offers to remove what it keeps in your Library too. Apps that are part of macOS are never removed."
                  : view === "leftovers"
                    ? "Named for an app that isn’t installed and unchanged for a month or more. Check each one: a tool without an app may still use its settings."
                    : "Disk images and packages in Downloads and on the Desktop. Installed apps run from Applications, not from these files."}
              </span>
              {view === "apps" ? (
                <FilterBar label="Filter apps" className="dev-list-note" state={filter} onChange={setFilter} items={apps} spec={appSpec} chips={appChips} />
              ) : (
                items.length > 0 && <FilterBar label={view === "leftovers" ? "Filter leftovers" : "Filter installers"} className="dev-list-note" state={filter} onChange={setFilter} items={items} spec={itemSpec} />
              )}
              {view === "apps" ? (
                <ul aria-label="Installed app list" className="dev-rows">
                  {shownApps.length === 0 && <li className="v2-muted">No apps match.</li>}
                  {shownApps.map((app) => (
                    <li key={app.id} className={focused?.id === app.id ? "focused" : ""}>
                      <span className="apps-spacer" aria-hidden="true" />
                      <button type="button" className="dev-row-main" aria-pressed={focused?.id === app.id} onClick={() => setFocus(app.id)}>
                        <Picture source="app" id={app.id} size={30} label={app.name} fallback={<IconTile tone={app.apple ? "gray" : "blue"} icon="apps" size={30} />} />
                        <span className="v2-row-text">
                          <span className="v2-row-title">{app.name}</span>
                          <span className="v2-row-sub v2-ellipsis">{[app.version && `Version ${app.version}`, app.location].filter(Boolean).join(" · ")}</span>
                        </span>
                      </button>
                      <span className="dev-size">{formatGb(gb(app.bytes + app.leftoverBytes))}</span>
                      <span className="dev-pill">
                        {app.apple ? <Pill status="muted">Part of macOS</Pill> : app.running ? <Pill status="warn">Open</Pill> : <Pill status="ok">Can remove</Pill>}
                      </span>
                    </li>
                  ))}
                </ul>
              ) : items.length === 0 ? (
                <span className="v2-card-note dev-list-note">{view === "leftovers" ? "No leftovers of removed apps were found." : "No installers were found in Downloads or on the Desktop."}</span>
              ) : (
                <ul aria-label={view === "leftovers" ? "Leftover list" : "Installer list"} className="dev-rows">
                  {shownItems.length === 0 && <li className="v2-muted">Nothing matches.</li>}
                  {shownItems.map((item) => {
                    const reviewId = item.reviewId;
                    const can = sendable(reviewId);
                    const on = can && selection.has(reviewId);
                    const installed = "installedApp" in item ? item.installedApp : null;
                    return (
                      <li key={reviewId ?? item.name}>
                        <input type="checkbox" aria-label={`Select ${item.name}`} checked={on} disabled={!can} onChange={() => toggle(reviewId)} style={{ opacity: can ? 1 : 0.4 }} />
                        <span className="dev-row-main">
                          <IconTile tone={view === "leftovers" ? "pink" : "orange"} icon={view === "leftovers" ? "folder" : "package"} size={30} />
                          <span className="v2-row-text">
                            <span className="v2-row-title v2-ellipsis">{item.name}</span>
                            <span className="v2-row-sub v2-ellipsis">{"owner" in item ? `${item.owner} · ${item.location}` : item.location}</span>
                          </span>
                        </span>
                        <span className="dev-size">{size(item)}</span>
                        <span className="dev-pill">
                          {view === "leftovers" ? (
                            <Pill status="warn">Check first</Pill>
                          ) : installed ? (
                            <Pill status="ok">{installed} installed</Pill>
                          ) : (
                            <Pill status="muted">Installer</Pill>
                          )}
                        </span>
                      </li>
                    );
                  })}
                </ul>
              )}
            </section>

            <div className="dev-side">
              {view === "apps" && focused && (
                <section aria-labelledby="apps-focus" className="v2-card dev-detail">
                  <div className="dev-detail-head">
                    <Picture source="app" id={focused.id} size={42} label={focused.name} fallback={<IconTile tone={focused.apple ? "gray" : "blue"} icon="apps" size={42} glow />} />
                    <span className="v2-row-text">
                      <h2 id="apps-focus" className="v2-card-title-sm v2-ellipsis">{focused.name}</h2>
                      <span className="v2-muted v2-num dev-detail-meta">
                        {size(focused)} app · {formatGb(gb(focused.leftoverBytes))} in your Library
                      </span>
                    </span>
                  </div>
                  {focused.leftovers.length > 0 ? (
                    <ul className="v2-info-list dev-parts">
                      {focused.leftovers.map((left) => (
                        <li key={`${left.location}/${left.name}`}>
                          <span className="v2-ellipsis">{left.location.replace(/^~\/Library\//, "")} › {left.name}</span>
                          <strong className="v2-num">{size(left)}</strong>
                        </li>
                      ))}
                    </ul>
                  ) : (
                    <span className="v2-card-note">Nothing found in your Library for this app.</span>
                  )}
                  {focused.bundleId && <span className="v2-mono v2-ellipsis dev-path">{focused.bundleId}</span>}
                  <div className="apps-actions">
                    {cacheBytes(focused) > 0 && (
                      <Button
                        variant="soft"
                        size="sm"
                        disabled={busy}
                        onClick={() => removal({ what: `${focused.name}’s caches`, prepare: () => props.onPrepareCaches([focused.id]), onDone: props.onRemoved })}
                      >
                        Clear caches ({formatGb(gb(cacheBytes(focused)))})
                      </Button>
                    )}
                    <Button variant="danger" size="sm" disabled={focused.apple || focused.running || busy} onClick={() => setConfirm(focused)}>
                      Remove {focused.name}…
                    </Button>
                    {focused.running && <span className="v2-card-note">Quit it first.</span>}
                  </div>
                </section>
              )}
              {failed && (
                <section aria-label="App Management" className="v2-card dev-detail">
                  <AccessSteps kind="appManagement" onOpen={props.onOpenAppManagement} />
                </section>
              )}
            </div>
          </div>
        )}
      </div>

      {result && view !== "apps" && (
        <FloatingBar>
          <div className="v2-float-text">
            <span role="status" className="v2-float-title">{picked.length > 0 ? `${picked.length} selected · ${formatGb(gb(pickedBytes))}` : "Nothing selected"}</span>
            <span className="v2-float-sub">You'll see exactly what will go before anything moves, and it goes to quarantine, so you can put it back.</span>
          </div>
          <div className="v2-float-actions">
            <Button variant="primary" disabled={picked.length === 0 || busy} onClick={removePicked}>
              {picked.length > 0 ? `Remove ${picked.length}…` : "Remove…"}
            </Button>
          </div>
        </FloatingBar>
      )}

      <Modal open={confirm !== null} onClose={() => setConfirm(null)} labelledBy="apps-remove-title" role="alertdialog">
        {confirm && (
          <div className="v2-dialog-body">
            <div className="v2-dialog-head">
              <Picture source="app" id={confirm.id} size={48} label={confirm.name} fallback={<IconTile tone="red" icon="trashLines" size={48} glow />} />
              <div className="v2-row-text">
                <h2 id="apps-remove-title" className="v2-dialog-title">Move {confirm.name} to the Trash?</h2>
                <span className="v2-card-note">Finder’s Put Back returns it until you empty the Trash.</span>
              </div>
            </div>
            <p className="v2-dialog-text">
              {confirm.leftovers.length > 0
                ? `Then Kleari shows you the ${plural(confirm.leftovers.length, "item")} it keeps in your Library (${formatGb(gb(confirm.leftoverBytes))}), so you can remove those too if you like.`
                : "It keeps nothing else in your Library that Kleari found."}
            </p>
            <div className="v2-dialog-actions">
              <Button variant="secondary" onClick={() => setConfirm(null)}>Cancel</Button>
              <Button variant="danger" disabled={busy} onClick={() => remove(confirm)}>
                {busy ? <Spinner size={10} tone="danger" /> : null} Move to Trash
              </Button>
            </div>
          </div>
        )}
      </Modal>

      <Toast message={toast.message} bottom={92} />
    </>
  );
}
