import { useCallback, useEffect, useState, type ReactNode } from "react";
import type { TrashFailure, TrashItem, TrashList } from "../../../contracts/trash";
import { runMaintenanceTask } from "../../../lib/maintenanceIpc";
import { openSystemSettings } from "../../../lib/systemPageIpc";
import { deleteTrashItems, listTrash, putBackTrashItems } from "../../../lib/trashIpc";
import { storageErrorText } from "../../../features/storage/format";
import { errorKind } from "../../../features/cleanup/useCleanupScan";
import { AccessSteps } from "../../ui/AccessSteps";
import { Button, FloatingBar, IconTile, Modal, PageHeader, Spinner, StatCard, Toast, useToast } from "../../ui/components";
import { formatGb, plural, sum } from "../../ui/format";
import { headingIdFor } from "../../shell/routes";
import { Picture } from "../../ui/Picture";
import { ImagePreview, type PreviewImage } from "../../ui/ImagePreview";
import { applyFilter, FilterBar, fileKind, fileKindChips, useListFilter, type ListFilterSpec } from "../../ui/ListFilter";
import "./trash.css";

const filterSpec: ListFilterSpec<TrashItem> = {
  bytes: (item) => item.bytes,
  name: (item) => item.name,
  text: (item) => item.from ?? "",
  date: (item) => item.trashedMs,
  type: fileKind,
};

const failureText: Record<TrashFailure, string> = {
  changed: "it changed or is gone since the list was read",
  noOrigin: "Finder didn't record where it came from",
  exists: "something is already where it came from",
  folderGone: "the folder it came from is gone",
  needsAccess: "macOS didn't give Kleari access",
  failed: "macOS reported an error",
};

/** Image files Kleari makes thumbnails of (src-tauri `files::images::EXTENSIONS`). */
const IMAGE_EXTENSIONS = /\.(jpe?g|png|heic|heif|tiff?|webp|dng|cr2|nef|arw)$/i;
const isImage = (item: TrashItem) => !item.folder && IMAGE_EXTENSIONS.test(item.name);

const size = (item: { bytes: number; lowerBound: boolean }) => `${item.lowerBound ? "≥ " : ""}${formatGb(item.bytes / 1e9)}`;

function when(ms: number | null): string {
  return ms === null ? "" : `deleted ${new Date(ms).toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" })}`;
}

/**
 * The Trash, inside Kleari: what is in it, Put Back, delete chosen items for good, and Empty
 * Trash. Read when the tab opens and after each change; never polled.
 */
export function TrashScreen({ tabs }: { tabs: ReactNode }) {
  const [list, setList] = useState<TrashList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [filter, setFilter] = useListFilter("trash");
  const [selection, setSelection] = useState<ReadonlySet<string>>(new Set());
  const [confirm, setConfirm] = useState<"delete" | "empty" | null>(null);
  // The images seen large: the Trash's images and the one chosen.
  const [preview, setPreview] = useState<{ images: PreviewImage[]; start: number } | null>(null);
  const toast = useToast(6000);

  const load = useCallback(() => {
    setLoading(true);
    listTrash().then(
      (next) => {
        setLoading(false);
        setError(null);
        setList(next);
        setSelection(new Set());
      },
      (failure: unknown) => {
        setLoading(false);
        setError(storageErrorText[errorKind(failure)]);
      },
    );
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const all = list?.status === "available" ? list.items : [];
  const items = applyFilter(all, filter, filterSpec);
  const picked = all.filter((item) => selection.has(item.id));
  const canPutBack = picked.filter((item) => item.from !== null);

  const toggle = (id: string) => {
    const next = new Set(selection);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    setSelection(next);
  };

  const finished = (verb: string, chosen: readonly TrashItem[], outcome: { done: string[]; bytes: number; failed: { id: string; reason: TrashFailure }[] }) => {
    const first = outcome.failed[0];
    const name = (id: string) => chosen.find((item) => item.id === id)?.name ?? "An item";
    if (outcome.failed.length === 0) {
      toast.show(`${verb} ${plural(outcome.done.length, "item")} (${formatGb(outcome.bytes / 1e9)}).`);
    } else {
      const rest = outcome.failed.length > 1 ? ` and ${plural(outcome.failed.length - 1, "other")}` : "";
      toast.show(`${verb} ${outcome.done.length} of ${chosen.length}. ${name(first.id)}${rest} stayed in the Trash: ${failureText[first.reason]}.`, "warn");
    }
    load();
  };

  const act = (verb: string, chosen: readonly TrashItem[], run: (ids: readonly string[]) => Promise<Parameters<typeof finished>[2]>) => {
    setBusy(true);
    run(chosen.map((item) => item.id)).then(
      (outcome) => {
        setBusy(false);
        finished(verb, chosen, outcome);
      },
      (failure: unknown) => {
        setBusy(false);
        toast.show(storageErrorText[errorKind(failure)], "warn");
      },
    );
  };

  const empty = () => {
    setBusy(true);
    runMaintenanceTask("emptyTrash").then(
      (outcome) => {
        setBusy(false);
        toast.show(outcome === "done" ? "The Trash is empty." : outcome === "cancelled" ? "Emptying the Trash was cancelled." : "Finder couldn’t empty the Trash.", outcome === "done" ? undefined : "warn");
        load();
      },
      (failure: unknown) => {
        setBusy(false);
        toast.show(storageErrorText[errorKind(failure)], "warn");
      },
    );
  };

  const total = list?.status === "available" ? list : null;

  return (
    <>
      <PageHeader
        title="Trash"
        headingId={headingIdFor("quarantine")}
        subtitle="What you’ve moved to the Trash, here and on external disks. Put things back, or delete them for good."
        actions={
          <>
            {tabs}
            <Button variant="soft" size="sm" disabled={loading} onClick={load}>{loading ? "Reading…" : "Refresh"}</Button>
            <Button variant="dangerOutline" size="sm" disabled={busy || all.length === 0} onClick={() => setConfirm("empty")}>Empty Trash…</Button>
          </>
        }
      />
      <div className={`trash-body${picked.length > 0 ? " trash-body-above-bar" : ""}`}>
        {error && <span role="alert" className="v2-card-note">{error}</span>}
        {list?.status === "notAllowed" ? (
          <section aria-labelledby="trash-access-title" className="v2-card trash-access">
            <div className="trash-access-head">
              <IconTile tone="blue" icon="lock" size={32} />
              <h2 id="trash-access-title" className="v2-card-title-sm">Kleari needs Full Disk Access to show the Trash</h2>
            </div>
            <AccessSteps kind="fullDiskAccess" compact />
            <div className="trash-access-actions">
              <Button variant="primary" size="sm" onClick={() => void openSystemSettings("fullDiskAccess")}>Open System Settings</Button>
              <Button variant="secondary" size="sm" onClick={load}>Check again</Button>
            </div>
          </section>
        ) : !list ? (
          <section aria-label="Reading" className="v2-card trash-state">
            <Spinner size={18} />
            <span role="status" className="v2-card-note">Reading the Trash…</span>
          </section>
        ) : all.length === 0 ? (
          <section aria-label="Empty" className="v2-card trash-state">
            <IconTile tone="green" icon="check" size={32} />
            <span className="v2-row-title">The Trash is empty</span>
            <span className="v2-card-note">What you move to the Trash in Finder shows here, from this Mac and from connected disks.</span>
          </section>
        ) : (
          <>
            <section aria-label="Trash summary" className="v2-grid-3">
              <StatCard tone="red" icon="trash" label="In the Trash" value={total ? size(total) : "—"} sub="Space it frees when emptied" />
              <StatCard tone="blue" icon="list" label="Items" value={String(all.length + (total?.more ?? 0))} sub={total && total.more > 0 ? `${total.more} more not listed` : "Across all disks"} />
              <StatCard tone="green" icon="arrowRight" label="Can go back" value={String(all.filter((item) => item.from !== null).length)} sub="Where Finder recorded the place" />
            </section>
            <section aria-labelledby="trash-list-title" className="v2-card trash-card">
              <div className="trash-head">
                <input
                  type="checkbox"
                  aria-label="Choose all shown"
                  checked={items.length > 0 && items.every((item) => selection.has(item.id))}
                  disabled={items.length === 0}
                  onChange={(event) => setSelection(event.target.checked ? new Set(items.map((item) => item.id)) : new Set())}
                />
                <h2 id="trash-list-title" className="v2-card-title-sm">In the Trash</h2>
                <span className="v2-muted v2-num">{items.length === all.length ? plural(all.length, "item") : `${items.length} of ${all.length} shown`}</span>
              </div>
              <FilterBar label="Filter the Trash" className="trash-tools" state={filter} onChange={setFilter} items={all} spec={filterSpec} chips={fileKindChips} />
              <ul className="trash-list" tabIndex={0} aria-label="Items in the Trash">
                {items.length === 0 && <li className="v2-muted">Nothing in the Trash matches.</li>}
                {items.map((item) => (
                  <li key={item.id}>
                    <input type="checkbox" aria-label={`Choose ${item.name}`} checked={selection.has(item.id)} onChange={() => toggle(item.id)} />
                    <Picture
                      source="trash"
                      id={item.id}
                      size={28}
                      label={item.name}
                      enabled={isImage(item)}
                      fallback={<IconTile tone={item.folder ? "blue" : "gray"} icon={item.folder ? "folder" : "file"} size={28} />}
                      onOpen={() => {
                        const images = items.filter(isImage);
                        setPreview({
                          images: images.map((other) => ({ id: other.id, name: other.name, detail: [other.from ? `from ${other.from}` : null, size(other)].filter(Boolean).join(" · ") })),
                          start: Math.max(0, images.findIndex((other) => other.id === item.id)),
                        });
                      }}
                    />
                    <span className="v2-row-text">
                      <span className="v2-row-title v2-ellipsis">{item.name}</span>
                      <span className="v2-row-sub v2-ellipsis">
                        {[item.from ? `from ${item.from}` : null, item.disk ? `on ${item.disk}` : null, when(item.trashedMs)].filter(Boolean).join(" · ")}
                      </span>
                    </span>
                    <span className="v2-num trash-size">{size(item)}</span>
                    <span className="trash-action">
                      {item.from !== null && (
                        <Button variant="soft" size="sm" disabled={busy} onClick={() => act("Put back", [item], putBackTrashItems)}>
                          Put Back
                        </Button>
                      )}
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          </>
        )}
      </div>

      {picked.length > 0 && (
        <FloatingBar>
          <div className="v2-float-text">
            <span role="status" className="v2-float-title">{picked.length} chosen · {formatGb(sum(picked, (item) => item.bytes) / 1e9)}</span>
            <span className="v2-float-sub">
              {canPutBack.length === picked.length ? "Put Back returns them where they came from." : `${plural(picked.length - canPutBack.length, "item")} can’t go back: Finder didn’t record where it came from.`}
            </span>
          </div>
          <div className="v2-float-actions">
            <Button variant="secondary" disabled={busy} onClick={() => setSelection(new Set())}>Clear</Button>
            <Button variant="soft" disabled={busy || canPutBack.length === 0} onClick={() => act("Put back", canPutBack, putBackTrashItems)}>
              Put Back {canPutBack.length > 0 ? canPutBack.length : ""}
            </Button>
            <Button variant="danger" disabled={busy} onClick={() => setConfirm("delete")}>Delete permanently</Button>
          </div>
        </FloatingBar>
      )}

      <Modal open={confirm !== null} onClose={() => setConfirm(null)} labelledBy="trash-confirm-title" role="alertdialog">
        {confirm && (
          <div className="v2-dialog-body">
            <h2 id="trash-confirm-title" className="v2-dialog-title">
              {confirm === "empty" ? "Empty the Trash?" : `Delete ${plural(picked.length, "item")} for good?`}
            </h2>
            <p className="v2-dialog-text">
              {confirm === "empty"
                ? `Everything in the Trash (${total ? size(total) : ""}), on every disk, is deleted for good. Finder does it.`
                : `${formatGb(sum(picked, (item) => item.bytes) / 1e9)} is deleted for good. It can’t be put back afterwards.`}
            </p>
            <div className="v2-dialog-actions">
              <Button variant="secondary" onClick={() => setConfirm(null)}>Cancel</Button>
              <Button
                variant="danger"
                onClick={() => {
                  const which = confirm;
                  setConfirm(null);
                  if (which === "empty") {
                    empty();
                  } else {
                    act("Deleted", picked, deleteTrashItems);
                  }
                }}
              >
                {confirm === "empty" ? "Empty Trash" : "Delete permanently"}
              </Button>
            </div>
          </div>
        )}
      </Modal>

      {preview && <ImagePreview source="trash" images={preview.images} start={preview.start} onClose={() => setPreview(null)} />}
      <Toast message={toast.message} bottom={92} />
    </>
  );
}
