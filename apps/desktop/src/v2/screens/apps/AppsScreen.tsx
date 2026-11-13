import { useState } from "react";
import type { AppEntry, AppsScan, InstallerEntry, LeftoverEntry } from "../../../contracts/apps";
import { Button, FloatingBar, IconTile, LinkButton, Modal, PageHeader, Pill, Segmented, Spinner, StatCard, Toast, useToast } from "../../ui/components";
import { formatGb, plural, sum } from "../../ui/format";
import { headingIdFor, hrefFor } from "../../shell/routes";
import "../developer/developer.css";
import "./apps.css";

type View = "apps" | "leftovers" | "installers";

interface AppsScreenProps {
  result: AppsScan | null;
  running: boolean;
  /** A message about the scan: stopped, or why it failed. */
  note: string | null;
  /** Review IDs already sent to Cleanup's review. */
  sent: ReadonlySet<string>;
  onScan: () => void;
  onCancel: () => void;
  /** Sends leftovers or installers to Cleanup step 2; resolves to a message. Nothing moves. */
  onSend: (ids: readonly string[]) => Promise<string>;
  /** Moves the app to the Trash; its leftovers go to the review. Resolves to a message. */
  onRemove: (app: AppEntry) => Promise<{ text: string; ok: boolean }>;
  onOpenAppManagement: () => void;
}

const gb = (bytes: number) => bytes / 1e9;

function size(item: { bytes: number; lowerBound?: boolean }): string {
  return `${item.lowerBound ? "≥ " : ""}${formatGb(gb(item.bytes))}`;
}

export function AppsScreen(props: AppsScreenProps) {
  const { result, running, sent } = props;
  const [view, setView] = useState<View>("apps");
  const [focus, setFocus] = useState<string | null>(null);
  const [selection, setSelection] = useState<ReadonlySet<string>>(new Set());
  const [confirm, setConfirm] = useState<AppEntry | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const toast = useToast(6000);

  const apps = result?.apps ?? [];
  const orphans = result?.orphans ?? [];
  const installers = result?.installers ?? [];
  const removable = apps.filter((app) => !app.apple);
  const focused = apps.find((app) => app.id === focus) ?? removable[0] ?? null;
  const sendable = (id: string | null): id is string => id !== null && !sent.has(id);
  const items: readonly (LeftoverEntry | InstallerEntry)[] = view === "installers" ? installers : orphans;
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

  const send = () => {
    setBusy(true);
    void props.onSend(picked.map((item) => item.reviewId ?? "")).then((message) => {
      setBusy(false);
      setSelection(new Set());
      toast.show(message);
    });
  };

  const remove = (app: AppEntry) => {
    setBusy(true);
    void props.onRemove(app).then((outcome) => {
      setBusy(false);
      setConfirm(null);
      setFailed(!outcome.ok);
      toast.show(outcome.text, outcome.ok ? undefined : "warn");
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
            <LinkButton href={hrefFor("review")} size="sm">Open review</LinkButton>
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
                  ? "Removing an app moves it to the Trash and sends what it keeps in your Library to Cleanup’s review. Apps that are part of macOS are never removed."
                  : view === "leftovers"
                    ? "Named for an app that isn’t installed and unchanged for a month or more. Check each one: a tool without an app may still use its settings."
                    : "Disk images and packages in Downloads and on the Desktop. Installed apps run from Applications, not from these files."}
              </span>
              {view === "apps" ? (
                <ul aria-label="Installed app list" className="dev-rows">
                  {apps.map((app) => (
                    <li key={app.id} className={focused?.id === app.id ? "focused" : ""}>
                      <span className="apps-spacer" aria-hidden="true" />
                      <button type="button" className="dev-row-main" aria-pressed={focused?.id === app.id} onClick={() => setFocus(app.id)}>
                        <IconTile tone={app.apple ? "gray" : "blue"} icon="apps" size={30} />
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
                  {items.map((item) => {
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
                          {reviewId !== null && sent.has(reviewId) ? (
                            <Pill status="link">In review</Pill>
                          ) : view === "leftovers" ? (
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
                    <IconTile tone={focused.apple ? "gray" : "blue"} icon="apps" size={42} glow />
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
                    <Button variant="danger" size="sm" disabled={focused.apple || focused.running || busy} onClick={() => setConfirm(focused)}>
                      Remove {focused.name}…
                    </Button>
                    {focused.running && <span className="v2-card-note">Quit it first.</span>}
                  </div>
                </section>
              )}
              {failed && (
                <section aria-label="App Management" className="v2-card dev-detail">
                  <span className="v2-card-note">macOS asks before one app removes another. Allow Kleari under App Management, then try again.</span>
                  <Button variant="soft" size="sm" onClick={props.onOpenAppManagement}>Open App Management settings</Button>
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
            <span className="v2-float-sub">Sending to review adds these to Cleanup step 2. Nothing moves until you approve it there, and then only into quarantine.</span>
          </div>
          <div className="v2-float-actions">
            <Button variant="primary" disabled={picked.length === 0 || busy} onClick={send}>
              {picked.length > 0 ? `Send ${picked.length} to review` : "Send to review"}
            </Button>
          </div>
        </FloatingBar>
      )}

      <Modal open={confirm !== null} onClose={() => setConfirm(null)} labelledBy="apps-remove-title" role="alertdialog">
        {confirm && (
          <div className="v2-dialog-body">
            <div className="v2-dialog-head">
              <IconTile tone="red" icon="trashLines" size={48} glow />
              <div className="v2-row-text">
                <h2 id="apps-remove-title" className="v2-dialog-title">Move {confirm.name} to the Trash?</h2>
                <span className="v2-card-note">Finder’s Put Back returns it until you empty the Trash.</span>
              </div>
            </div>
            <p className="v2-dialog-text">
              {confirm.leftovers.length > 0
                ? `Then the ${plural(confirm.leftovers.length, "item")} it keeps in your Library (${formatGb(gb(confirm.leftoverBytes))}) go to Cleanup’s review. They move only when you approve them there, into quarantine, where you can restore them.`
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

      <Toast message={toast.message} bottom={92}>
        <a className="v2-link dev-toast-link" href={hrefFor("review")}>Open review</a>
      </Toast>
    </>
  );
}
