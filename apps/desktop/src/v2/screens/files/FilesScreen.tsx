import { useState } from "react";
import { tools, type FileGroup, type FileItem, type FilesOptions, type Tool } from "../../../contracts/files";
import type { FileCategory } from "../../../contracts/storage";
import type { Files } from "../../../features/files/useFiles";
import type { StorageExplorer as Explorer } from "../../../features/storage/useStorageExplorer";
import type { IconName } from "../../ui/icons";
import { Button, FloatingBar, IconTile, PageHeader, Pill, Segmented, Select, Spinner, type Tone } from "../../ui/components";
import { formatGb, plural, sum } from "../../ui/format";
import { headingIdFor, tabFromHash } from "../../shell/routes";
import { useRemoval } from "../../../features/removal/RemovalProvider";
import { Picture } from "../../ui/Picture";
import { ImagePreview, type PreviewImage } from "../../ui/ImagePreview";
import { applyFilter, FilterBar, fileKind, fileKindChips, useListFilter, type ListFilterSpec } from "../../ui/ListFilter";
import { StorageExplorer } from "../storage/StorageExplorer";
import "../storage/storage.css";
import "./files.css";

export const toolMeta: Record<Tool, { label: string; tone: Tone; icon: IconName; what: string; group: (group: FileGroup) => string }> = {
  largeOld: {
    label: "Large & Old",
    tone: "orange",
    icon: "file",
    what: "Big files in your home folder, and files of 50 MB or more you haven’t opened or changed in a long time.",
    group: (group) => (group.kind === "large" ? "Large files" : "Not opened in a long time"),
  },
  duplicates: {
    label: "Duplicates",
    tone: "violet",
    icon: "copy",
    what: "Files of 1 MB or more with exactly the same contents. One copy of each is always kept: by default the oldest. Space saved is “up to”: copies macOS made as clones share their space, so removing one may free less.",
    group: (group) => `${plural(group.items.length, "copy", "copies")}`,
  },
  similar: {
    label: "Similar Images",
    tone: "pink",
    icon: "eye",
    what: "Photos and pictures that look alike, such as burst shots or resized copies. The largest of each group is kept by default.",
    group: (group) => `${plural(group.items.length, "image")} that look alike`,
  },
  downloads: {
    label: "Downloads",
    tone: "blue",
    icon: "download",
    what: "What is in your Downloads folder, by the app that downloaded it (Safari, Chrome, Slack, …), as macOS tagged it.",
    group: (group) => group.label,
  },
  ai: {
    label: "AI clutter",
    tone: "teal",
    icon: "bolt",
    what: "Models AI tools downloaded (Ollama, Hugging Face, LM Studio, Whisper, …) and AI apps’ caches and logs.",
    group: (group) => group.label,
  },
};

const fileIcon: Record<FileCategory, IconName> = {
  video: "film",
  image: "file",
  audio: "file",
  archive: "archive",
  diskImage: "disk",
  document: "file",
  code: "code",
  other: "file",
};

const gb = (bytes: number) => bytes / 1e9;

function size(item: { bytes: number; lowerBound?: boolean }): string {
  return `${item.lowerBound ? "≥ " : ""}${formatGb(gb(item.bytes))}`;
}

function age(ms: number | null, nowMs: number): string {
  if (ms === null) {
    return "date not reported";
  }
  const days = Math.floor(Math.max(0, nowMs - ms) / 86_400_000);
  if (days < 1) {
    return "today";
  }
  if (days < 60) {
    return `${plural(days, "day")} ago`;
  }
  const months = Math.floor(days / 30);
  return months < 24 ? `${plural(months, "month")} ago` : `${Math.floor(days / 365)} years ago`;
}

/** In a group of copies or look-alikes, the first item is kept and can't be chosen. */
function keeps(group: FileGroup): boolean {
  return group.kind === "copies" || group.kind === "similar";
}

/** The item a group keeps: its first as found, whatever order it is shown in. */
function isKept(group: FileGroup, item: FileItem): boolean {
  return keeps(group) && group.items[0]?.id === item.id;
}

const filterSpec: ListFilterSpec<FileItem> = {
  bytes: (item) => item.bytes,
  name: (item) => item.name,
  text: (item) => item.location,
  date: (item) => item.modifiedMs,
  type: fileKind,
};

export function FilesScreen({ files, explorer }: { files: Files; explorer: Explorer }) {
  const { state } = files;
  const remove = useRemoval();
  const [tool, setTool] = useState<Tool>(() => tabFromHash(tools, "largeOld"));
  // A folder of Downloads opened in place (the Browse view), and why one didn't open.
  const [browsing, setBrowsing] = useState(false);
  const [folderNote, setFolderNote] = useState<string | null>(null);
  const [options, setOptions] = useState<FilesOptions>({ largeMinMb: 500, oldDays: 180 });
  const [selection, setSelection] = useState<ReadonlySet<string>>(new Set());
  // The images seen large: a group's images and the one chosen.
  const [preview, setPreview] = useState<{ images: PreviewImage[]; start: number } | null>(null);
  const nowMs = Date.now();

  const [filter, setFilter] = useListFilter(`files.${tool}`);
  const result = state.results[tool] ?? null;
  const meta = toolMeta[tool];
  const groups = result?.groups ?? [];
  const everything = groups.flatMap((group) => group.items);
  // Each group shows what matches, in the chosen order; a group with nothing left is hidden.
  // The type chips count every group, so each group is filtered against all of them.
  const shownGroups = groups.map((group) => ({ group, items: applyFilter(group.items, filter, filterSpec, everything) })).filter((shown) => shown.items.length > 0);
  const choosable = (group: FileGroup, item: FileItem) => item.selectable && !isKept(group, item);
  const picked = groups.flatMap((group) => group.items.filter((item) => choosable(group, item) && selection.has(item.id)));
  const running = state.running === tool;

  const toggle = (id: string) => {
    const next = new Set(selection);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    setSelection(next);
  };

  /** Chooses everything shown but the copy each group keeps. */
  const chooseAllButKept = () => {
    const ids = shownGroups.flatMap(({ group, items }) => items.filter((item) => choosable(group, item)).map((item) => item.id));
    setSelection(new Set(ids));
  };

  const openFolder = (item: FileItem) => {
    setFolderNote(null);
    files.openFolder(item.id).then(
      (id) => {
        if (id === null) {
          setFolderNote(`“${item.name}” changed since the search. Search again to open it.`);
          return;
        }
        setSelection(new Set());
        setBrowsing(true);
        explorer.open(id);
      },
      () => setFolderNote(`Kleari couldn’t open “${item.name}”.`),
    );
  };

  const removePicked = () => {
    const ids = picked.map((item) => item.id);
    remove({
      what: plural(ids.length, "file"),
      // A copy may be an APFS clone sharing its space with the one kept.
      freesUpTo: tool === "duplicates",
      prepare: () => files.prepare(ids),
      onDone: () => {
        setSelection(new Set());
        files.rerun();
      },
    });
  };

  return (
    <>
      <PageHeader
        title="Files"
        headingId={headingIdFor("files")}
        subtitle="Find your own files worth a look. Nothing moves until you approve it, and then only into quarantine."
      />
      <div className="files-body">
        <Segmented
          label="Tool"
          value={tool}
          onChange={(next) => {
            setTool(next);
            setSelection(new Set());
            setBrowsing(false);
            setFolderNote(null);
          }}
          options={(Object.keys(toolMeta) as Tool[]).map((value) => ({ value, label: toolMeta[value].label }))}
        />
        <section aria-labelledby="files-tool" className="v2-card files-tool">
          <IconTile tone={meta.tone} icon={meta.icon} size={42} glow />
          <div className="v2-row-text">
            <h2 id="files-tool" className="v2-card-title-sm">{meta.label}</h2>
            <span className="v2-card-note">{meta.what}</span>
            {result && (
              <span className="v2-card-note">
                {plural(result.filesSeen, "item")} looked at{result.unreadable > 0 ? ` · ${plural(result.unreadable, "folder")} macOS didn’t let Kleari open` : ""}
                {result.truncated ? " · stopped at 3 million files" : ""}
              </span>
            )}
          </div>
          <div className="files-tool-actions">
            {tool === "largeOld" && (
              <>
                <Select
                  label="Large from"
                  value={String(options.largeMinMb)}
                  options={["100", "500", "1000", "5000"].map((value) => ({ value, label: value === "1000" ? "1 GB" : value === "5000" ? "5 GB" : `${value} MB` }))}
                  onChange={(value) => setOptions({ ...options, largeMinMb: Number(value) as FilesOptions["largeMinMb"] })}
                />
                <Select
                  label="Old after"
                  value={String(options.oldDays)}
                  options={[
                    { value: "90", label: "3 months" },
                    { value: "180", label: "6 months" },
                    { value: "365", label: "1 year" },
                    { value: "730", label: "2 years" },
                  ]}
                  onChange={(value) => setOptions({ ...options, oldDays: Number(value) as FilesOptions["oldDays"] })}
                />
              </>
            )}
            {running ? (
              <Button variant="secondary" size="sm" onClick={files.cancel}>
                <Spinner size={10} /> Stop
              </Button>
            ) : (
              <Button variant="primary" size="sm" disabled={state.running !== null} onClick={() => files.run(tool, options)}>
                {result ? "Search again" : "Search"}
              </Button>
            )}
          </div>
        </section>
        {(state.error || state.stopped) && <span role="status" className="v2-card-note">{state.error ?? "The search was stopped. Showing the last full result."}</span>}
        {folderNote && <span role="status" className="v2-card-note">{folderNote}</span>}
        {!browsing && groups.length > 0 && (
          <FilterBar label={`Filter ${meta.label}`} state={filter} onChange={setFilter} items={everything} spec={filterSpec} chips={fileKindChips} />
        )}
        {/* The tabs and the tool stay in place; only what was found scrolls. */}
        <div className="files-results" tabIndex={0} role="region" aria-label="What was found">
          {browsing ? (
            <section aria-label="Folder in Downloads" className="v2-card files-browse">
              <div>
                <Button variant="soft" size="sm" onClick={() => setBrowsing(false)}>‹ Back to Downloads</Button>
              </div>
              <StorageExplorer explorer={explorer} intro="Open folders inside it and keep going deeper. Choose what to remove and you'll see it all again first." />
            </section>
          ) : (
          <>
          {running && !result && (
            <section className="v2-card files-empty" aria-label="Searching">
              <Spinner size={30} />
              <span className="v2-row-title">Searching…</span>
              <span className="v2-card-note">{tool === "duplicates" || tool === "similar" ? "Comparing files can take a few minutes on a full home folder." : "This only reads names, sizes and dates."}</span>
            </section>
          )}
          {result && groups.length === 0 && (
            <section className="v2-card files-empty" aria-label="Nothing found">
              <IconTile tone="green" icon="check" size={36} />
              <span className="v2-row-title">Nothing found</span>
            </section>
          )}
          {groups.length > 0 && shownGroups.length === 0 && (
            <section className="v2-card files-empty" aria-label="Nothing matches">
              <IconTile tone="gray" icon="search" size={36} />
              <span className="v2-row-title">Nothing found matches</span>
              <span className="v2-card-note">Clear the search or pick another type.</span>
            </section>
          )}
          {shownGroups.map(({ group, items }) => (
            <section key={group.id} aria-label={meta.group(group)} className="v2-card files-group">
              <div className="files-group-head">
                <span className="v2-row-text">
                  <span className="v2-row-title">{meta.group(group)}</span>
                  {group.detail && <span className="v2-row-sub">{group.detail}</span>}
                </span>
                <span className="v2-num v2-muted">
                  {group.kind === "copies" ? `up to ${formatGb(gb(group.reclaimable))} to gain` : keeps(group) ? `${formatGb(gb(group.reclaimable))} to gain` : formatGb(gb(group.bytes))}
                </span>
              </div>
              <ul className="files-rows">
                {items.map((item) => {
                  const can = choosable(group, item);
                  const kept = isKept(group, item);
                  return (
                    <li key={item.id}>
                      <input type="checkbox" aria-label={`Choose ${item.name}`} checked={can && selection.has(item.id)} disabled={!can} onChange={() => toggle(item.id)} style={{ opacity: can ? 1 : 0.4 }} />
                      <Picture
                        source="files"
                        id={item.id}
                        size={28}
                        label={item.name}
                        enabled={item.fileCategory === "image"}
                        fallback={<IconTile tone={meta.tone} icon={item.folder ? "folder" : fileIcon[item.fileCategory ?? "other"]} size={28} />}
                        onOpen={() => {
                          const images = items.filter((entry) => entry.fileCategory === "image");
                          setPreview({
                            images: images.map((entry) => ({
                              id: entry.id,
                              name: entry.name,
                              detail: `${entry.location} · ${size(entry)}`,
                              note: isKept(group, entry) ? "Keep" : undefined,
                            })),
                            start: Math.max(0, images.findIndex((entry) => entry.id === item.id)),
                          });
                        }}
                      />
                      <span className="v2-row-text">
                        {tool === "downloads" && item.folder ? (
                          <button type="button" className="v2-row-title v2-ellipsis v2-link files-open" title={`Open ${item.name}`} onClick={() => openFolder(item)}>
                            {item.name} ›
                          </button>
                        ) : (
                          <span className="v2-row-title v2-ellipsis">{item.name}</span>
                        )}
                        <span className="v2-row-sub v2-ellipsis">
                          {item.location} · {tool === "largeOld" && group.kind === "old" ? `opened ${age(item.openedMs ?? item.modifiedMs, nowMs)}` : `changed ${age(item.modifiedMs, nowMs)}`}
                        </span>
                      </span>
                      <span className="v2-num files-size">{size(item)}</span>
                      <span className="files-pill">
                        {kept ? <Pill status="ok">Keep</Pill> : !item.selectable ? <Pill status="muted">Kept as is</Pill> : null}
                      </span>
                    </li>
                  );
                })}
                {group.more > 0 && <li className="v2-muted files-more">and {plural(group.more, "smaller item")}</li>}
              </ul>
            </section>
          ))}
          </>
          )}
        </div>
      </div>

      {!browsing && result && groups.length > 0 && (
        <FloatingBar>
          <div className="v2-float-text">
            <span role="status" className="v2-float-title">{picked.length > 0 ? `${picked.length} chosen · ${tool === "duplicates" ? "up to " : ""}${formatGb(gb(sum(picked, (item) => item.bytes)))}` : "Nothing chosen"}</span>
            <span className="v2-float-sub">You'll see exactly what will go before anything moves, and it goes to quarantine, so you can put it back.</span>
          </div>
          <div className="v2-float-actions">
            {keeps(groups[0]) && <Button variant="secondary" onClick={chooseAllButKept}>Choose all but one of each</Button>}
            <Button variant="primary" disabled={picked.length === 0} onClick={removePicked}>
              {picked.length > 0 ? `Remove ${picked.length}…` : "Remove…"}
            </Button>
          </div>
        </FloatingBar>
      )}

      {preview && <ImagePreview source="files" images={preview.images} start={preview.start} onClose={() => setPreview(null)} />}
    </>
  );
}
