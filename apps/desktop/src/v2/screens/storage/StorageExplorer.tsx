import { useState } from "react";
import type { ExplorerEntry } from "../../../contracts/storageExplorer";
import type { FileCategory } from "../../../contracts/storage";
import type { StorageExplorer as Explorer } from "../../../features/storage/useStorageExplorer";
import type { IconName } from "../../ui/icons";
import { Button, IconTile, Pill, Spinner, useToast, Toast, type Tone } from "../../ui/components";
import { formatGb, plural, sum } from "../../ui/format";
import { hrefFor } from "../../shell/routes";

const fileLook: Record<FileCategory, { icon: IconName; tone: Tone }> = {
  video: { icon: "film", tone: "pink" },
  image: { icon: "file", tone: "violet" },
  audio: { icon: "file", tone: "indigo" },
  archive: { icon: "archive", tone: "orange" },
  diskImage: { icon: "disk", tone: "orange" },
  document: { icon: "file", tone: "blue" },
  code: { icon: "code", tone: "green" },
  other: { icon: "file", tone: "gray" },
};

/** Map colours, in order, for the largest entries. */
const mapTones: readonly Tone[] = ["blue", "violet", "pink", "orange", "teal", "green", "indigo", "yellow"];

function look(entry: ExplorerEntry): { icon: IconName; tone: Tone } {
  if (entry.kind === "folder") {
    return { icon: "folder", tone: "blue" };
  }
  if (entry.kind === "file") {
    return fileLook[entry.fileCategory ?? "other"];
  }
  return { icon: entry.kind === "excluded" ? "eyeOff" : "arrowRight", tone: "gray" };
}

const gb = (bytes: number) => bytes / 1e9;

function size(entry: { bytes: number; lowerBound: boolean }): string {
  return `${entry.lowerBound ? "≥ " : ""}${formatGb(gb(entry.bytes))}`;
}

/**
 * Browse the home folder one folder at a time, down to single files: a map of what takes the
 * most space, every entry measured, and a selection to send to Cleanup's review.
 */
export function StorageExplorer({ explorer }: { explorer: Explorer }) {
  const { state, sent } = explorer;
  const { listing } = state;
  const [selection, setSelection] = useState<ReadonlySet<string>>(new Set());
  const [sending, setSending] = useState(false);
  const toast = useToast(5000);

  const entries = listing?.entries ?? [];
  const can = (entry: ExplorerEntry) => entry.selectable && !sent.has(entry.id);
  const picked = entries.filter((entry) => can(entry) && selection.has(entry.id));
  const total = Math.max(listing?.bytes ?? 0, 1);
  const mapped = entries.filter((entry) => entry.bytes / total >= 0.01).slice(0, 12);
  const mappedBytes = sum(mapped, (entry) => entry.bytes);

  const open = (id: string | null) => {
    setSelection(new Set());
    explorer.open(id);
  };

  const toggle = (entry: ExplorerEntry) => {
    if (!can(entry)) {
      return;
    }
    const next = new Set(selection);
    if (next.has(entry.id)) {
      next.delete(entry.id);
    } else {
      next.add(entry.id);
    }
    setSelection(next);
  };

  const send = () => {
    setSending(true);
    void explorer.send(picked.map((entry) => entry.id)).then((message) => {
      setSending(false);
      setSelection(new Set());
      toast.show(message);
    });
  };

  return (
    <div className="st-browse">
      <div className="st-browse-head">
        <span className="v2-card-note">
          Open any folder in your home folder, down to single files. What you send to review moves only when you approve it there, into quarantine, where you can restore it.
        </span>
        <div className="st-browse-actions">
          {state.loading ? (
            <Button variant="secondary" size="sm" onClick={explorer.cancel}>
              <Spinner size={10} /> Stop
            </Button>
          ) : (
            listing && <Button variant="soft" size="sm" onClick={() => open(listing.id)}>Measure again</Button>
          )}
        </div>
      </div>
      {listing && (
        <nav aria-label="Folder path" className="st-crumbs">
          {listing.crumbs.map((crumb, index) => (
            <span key={crumb.id} className="st-crumb">
              {index > 0 && <span aria-hidden="true">›</span>}
              {index === listing.crumbs.length - 1 ? (
                <strong aria-current="page">{crumb.name}</strong>
              ) : (
                <button type="button" className="v2-link" onClick={() => open(crumb.id)}>{crumb.name}</button>
              )}
            </span>
          ))}
          <span className="v2-muted v2-num">{size(listing)}</span>
        </nav>
      )}

      {(state.loading || state.error || listing?.status === "unreadable" || listing?.status === "changed") && (
        <span role="status" className="v2-card-note">
          {state.loading
            ? state.progress && state.progress.total > 0
              ? `Measuring ${state.progress.done} of ${plural(state.progress.total, "item")}…`
              : "Measuring…"
            : state.error ??
              (listing?.status === "unreadable"
                ? "macOS didn’t let Kleari open this folder. Full Disk Access may be needed."
                : "This folder changed since it was listed. Go up a level and measure again.")}
        </span>
      )}

      {!listing && !state.loading && (
        <div className="st-empty">
          <IconTile tone="blue" icon="home" size={42} glow />
          <Button variant="primary" size="sm" onClick={() => open(null)}>Browse Home</Button>
        </div>
      )}

      {listing && mapped.length > 0 && (
        <div className="st-map" role="list" aria-label="Storage map">
          {mapped.map((entry, index) => (
            <button
              key={entry.id}
              type="button"
              role="listitem"
              className={`st-map-cell tone-${mapTones[index % mapTones.length]}`}
              style={{ flexGrow: entry.bytes / mappedBytes }}
              title={`${entry.name} · ${size(entry)}`}
              disabled={entry.kind !== "folder" || state.loading}
              onClick={() => open(entry.id)}
            >
              <span className="v2-ellipsis">{entry.name}</span>
              <span className="v2-num">{formatGb(gb(entry.bytes))}</span>
            </button>
          ))}
        </div>
      )}

      {listing && (
        <ul aria-label="Folder contents" className="st-browse-rows">
          {entries.map((entry) => {
            const { icon, tone } = look(entry);
            const choosable = can(entry);
            return (
              <li key={entry.id}>
                <input type="checkbox" aria-label={`Choose ${entry.name}`} checked={choosable && selection.has(entry.id)} disabled={!choosable} onChange={() => toggle(entry)} style={{ opacity: choosable ? 1 : 0.4 }} />
                <IconTile tone={tone} icon={icon} size={28} />
                {entry.kind === "folder" ? (
                  <button type="button" className="st-browse-name v2-link" disabled={state.loading} onClick={() => open(entry.id)}>
                    <span className="v2-ellipsis">{entry.name}</span>
                    <span className="v2-row-sub">{plural(entry.files, "file")}</span>
                  </button>
                ) : (
                  <span className="st-browse-name">
                    <span className="v2-ellipsis">{entry.name}</span>
                    <span className="v2-row-sub">{entry.kind === "excluded" ? "Excluded in Settings" : entry.kind === "link" ? "Link, not followed" : "File"}</span>
                  </span>
                )}
                <span className="st-browse-bar" aria-hidden="true"><span style={{ width: `${Math.min(100, (entry.bytes / total) * 100).toFixed(1)}%` }} /></span>
                <span className="v2-num st-browse-size">{entry.kind === "excluded" || entry.kind === "link" ? "—" : size(entry)}</span>
                <span className="st-browse-pill">
                  {sent.has(entry.id) ? <Pill status="link">In review</Pill> : !entry.selectable && (entry.kind === "folder" || entry.kind === "file") ? <Pill status="muted">Kept as is</Pill> : null}
                </span>
              </li>
            );
          })}
          {listing.moreEntries > 0 && (
            <li className="v2-muted">
              <span />
              <span />
              <span className="st-browse-name">{plural(listing.moreEntries, "smaller item")}</span>
              <span />
              <span className="v2-num st-browse-size">{formatGb(gb(listing.moreBytes))}</span>
              <span />
            </li>
          )}
          {entries.length === 0 && <li className="v2-muted"><span /><span /><span className="st-browse-name">This folder is empty.</span></li>}
        </ul>
      )}

      {listing && (
        <div className="st-browse-foot">
          <span role="status" className="v2-card-note">
            {picked.length > 0 ? `${picked.length} chosen · ${formatGb(gb(sum(picked, (entry) => entry.bytes)))}` : "“Kept as is” items are places Kleari never moves: top-level folders, most of the Library and the inside of app packages."}
          </span>
          <Button variant="primary" size="sm" disabled={picked.length === 0 || sending} onClick={send}>
            {picked.length > 0 ? `Send ${picked.length} to review` : "Send to review"}
          </Button>
        </div>
      )}

      <Toast message={toast.message}>
        <a className="v2-link" href={hrefFor("review")}>Open review</a>
      </Toast>
    </div>
  );
}
