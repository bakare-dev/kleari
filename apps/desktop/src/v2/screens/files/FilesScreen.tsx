import { useState } from "react";
import type { FileGroup, FileItem, FilesOptions, Tool } from "../../../contracts/files";
import type { FileCategory } from "../../../contracts/storage";
import type { Files } from "../../../features/files/useFiles";
import type { IconName } from "../../ui/icons";
import { Button, FloatingBar, IconTile, LinkButton, PageHeader, Pill, Segmented, Select, Spinner, Toast, useToast, type Tone } from "../../ui/components";
import { formatGb, plural, sum } from "../../ui/format";
import { headingIdFor, hrefFor } from "../../shell/routes";
import "./files.css";

const toolMeta: Record<Tool, { label: string; tone: Tone; icon: IconName; what: string; group: (group: FileGroup) => string }> = {
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
    what: "Files of 1 MB or more with exactly the same contents. One copy of each is always kept: by default the oldest.",
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

export function FilesScreen({ files }: { files: Files }) {
  const { state, sent } = files;
  const [tool, setTool] = useState<Tool>("largeOld");
  const [options, setOptions] = useState<FilesOptions>({ largeMinMb: 500, oldDays: 180 });
  const [selection, setSelection] = useState<ReadonlySet<string>>(new Set());
  const [sending, setSending] = useState(false);
  const toast = useToast(6000);
  const nowMs = Date.now();

  const result = state.results[tool] ?? null;
  const meta = toolMeta[tool];
  const groups = result?.groups ?? [];
  const choosable = (group: FileGroup, item: FileItem, index: number) => item.selectable && !sent.has(item.id) && !(keeps(group) && index === 0);
  const picked = groups.flatMap((group) => group.items.filter((item, index) => choosable(group, item, index) && selection.has(item.id)));
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

  const chooseAllButKept = () => {
    const ids = groups.flatMap((group) => group.items.filter((item, index) => choosable(group, item, index)).map((item) => item.id));
    setSelection(new Set(ids));
  };

  const send = () => {
    setSending(true);
    void files.send(picked.map((item) => item.id)).then((message) => {
      setSending(false);
      setSelection(new Set());
      toast.show(message);
    });
  };

  return (
    <>
      <PageHeader
        title="Files"
        headingId={headingIdFor("files")}
        subtitle="Find your own files worth a look. Nothing moves until you approve it in Cleanup, and then only into quarantine."
        actions={<LinkButton href={hrefFor("review")} size="sm">Open review</LinkButton>}
      />
      <div className={`files-body${result ? " files-body-above-bar" : ""}`}>
        <Segmented
          label="Tool"
          value={tool}
          onChange={(next) => {
            setTool(next);
            setSelection(new Set());
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
        {groups.map((group) => (
          <section key={group.id} aria-label={meta.group(group)} className="v2-card files-group">
            <div className="files-group-head">
              <span className="v2-row-text">
                <span className="v2-row-title">{meta.group(group)}</span>
                {group.detail && <span className="v2-row-sub">{group.detail}</span>}
              </span>
              <span className="v2-num v2-muted">{keeps(group) ? `${formatGb(gb(group.reclaimable))} to gain` : formatGb(gb(group.bytes))}</span>
            </div>
            <ul className="files-rows">
              {group.items.map((item, index) => {
                const can = choosable(group, item, index);
                const kept = keeps(group) && index === 0;
                return (
                  <li key={item.id}>
                    <input type="checkbox" aria-label={`Choose ${item.name}`} checked={can && selection.has(item.id)} disabled={!can} onChange={() => toggle(item.id)} style={{ opacity: can ? 1 : 0.4 }} />
                    <IconTile tone={meta.tone} icon={item.folder ? "folder" : fileIcon[item.fileCategory ?? "other"]} size={28} />
                    <span className="v2-row-text">
                      <span className="v2-row-title v2-ellipsis">{item.name}</span>
                      <span className="v2-row-sub v2-ellipsis">
                        {item.location} · {tool === "largeOld" && group.kind === "old" ? `opened ${age(item.openedMs ?? item.modifiedMs, nowMs)}` : `changed ${age(item.modifiedMs, nowMs)}`}
                      </span>
                    </span>
                    <span className="v2-num files-size">{size(item)}</span>
                    <span className="files-pill">
                      {sent.has(item.id) ? <Pill status="link">In review</Pill> : kept ? <Pill status="ok">Keep</Pill> : !item.selectable ? <Pill status="muted">Kept as is</Pill> : null}
                    </span>
                  </li>
                );
              })}
              {group.more > 0 && <li className="v2-muted files-more">and {plural(group.more, "smaller item")}</li>}
            </ul>
          </section>
        ))}
      </div>

      {result && groups.length > 0 && (
        <FloatingBar>
          <div className="v2-float-text">
            <span role="status" className="v2-float-title">{picked.length > 0 ? `${picked.length} chosen · ${formatGb(gb(sum(picked, (item) => item.bytes)))}` : "Nothing chosen"}</span>
            <span className="v2-float-sub">Sending to review adds these to Cleanup step 2. Nothing moves until you approve it there, and then only into quarantine.</span>
          </div>
          <div className="v2-float-actions">
            {keeps(groups[0]) && <Button variant="secondary" onClick={chooseAllButKept}>Choose all but one of each</Button>}
            <Button variant="primary" disabled={picked.length === 0 || sending} onClick={send}>
              {picked.length > 0 ? `Send ${picked.length} to review` : "Send to review"}
            </Button>
          </div>
        </FloatingBar>
      )}

      <Toast message={toast.message} bottom={92}>
        <a className="v2-link" href={hrefFor("review")}>Open review</a>
      </Toast>
    </>
  );
}
