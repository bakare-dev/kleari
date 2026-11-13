import { useRef, useState } from "react";
import type { IconName } from "../../ui/icons";
import { Button, FloatingBar, IconTile, LinkButton, PageHeader, Pill, ScrollEdgeGlass, Segmented, Spinner, StatCard, Toast, useScrolled, useToast, type Status, type Tone } from "../../ui/components";
import { formatGb, plural, sum } from "../../ui/format";
import { headingIdFor, hrefFor } from "../../shell/routes";
import "./developer.css";

/** How careful to be: low and check can be sent to review; Xcode-managed and active never can.
 * Cleanup decides again in Rust what may move. */
export type DevRisk = "low" | "check" | "xcode" | "active";

export interface DevEntry {
  id: string;
  name: string;
  sub: string;
  gb: number;
  /** Something inside couldn't be read or counted, so the size is a minimum. */
  lowerBound?: boolean;
  risk: DevRisk;
  tone: Tone;
  icon: IconName;
  path: string;
  meta: string;
  what: string;
  after: string;
  parts: readonly { name: string; gb: number }[];
  /** What Send to review sends: the cache, or each of a project's build folders. Empty when
   * the backend couldn't identify it, so it can't be sent. */
  reviewIds: readonly string[];
}

export type DockerView =
  | { status: "running"; totalGb: number; complete: boolean; imageGb: number | null; parts: readonly { label: string; gb: number; tone: Tone }[] }
  | { status: "notRunning"; imageGb: number | null }
  | { status: "notInstalled" }
  | { status: "notChecked" };

interface DeveloperScreenProps {
  toolchains: readonly DevEntry[];
  projects: readonly DevEntry[];
  /** `null` until a scan has finished. */
  developerGb: number | null;
  /** The total includes a minimum, so it is shown as one. */
  developerLowerBound?: boolean;
  shareOfUsed: string | null;
  rebuildsGb: number | null;
  projectsGb: number | null;
  projectsLowerBound?: boolean;
  projectsNote: string;
  docker: DockerView | null;
  status: { running: boolean; text: string | null; failed: string | null; note: string | null };
  onScan: () => void;
  onCancel: () => void;
  /** Opens the macOS folder panel; resolves to a message to show, if any. */
  onAddFolder: () => Promise<string | null>;
  /** Review IDs already sent to Cleanup's review. */
  sent: ReadonlySet<string>;
  /** Sends the entries to Cleanup step 2; resolves to a message. Nothing moves. */
  onSendToReview: (entries: readonly DevEntry[]) => Promise<string>;
}

/** Every one of the entry's items is already in review. */
function inReview(entry: DevEntry, sent: ReadonlySet<string>): boolean {
  return entry.reviewIds.length > 0 && entry.reviewIds.every((id) => sent.has(id));
}

const riskPill: Record<DevRisk, { label: string; status: Status }> = {
  low: { label: "Low risk", status: "ok" },
  check: { label: "Check first", status: "warn" },
  xcode: { label: "Remove in Xcode", status: "muted" },
  active: { label: "Active project", status: "muted" },
};

/** Reclaiming Docker space is Docker's job; Kleari only shows the command (ADR-005). */
const DOCKER_COMMAND = "docker system prune";

type View = "toolchains" | "projects";

function size(entry: { gb: number; lowerBound?: boolean }): string {
  return `${entry.lowerBound ? "≥ " : ""}${formatGb(entry.gb)}`;
}

export function DeveloperScreen(props: DeveloperScreenProps) {
  const { toolchains, projects, status, sent } = props;
  const [view, setView] = useState<View>("toolchains");
  const [focus, setFocus] = useState<string | null>(null);
  const [selection, setSelection] = useState<ReadonlySet<string>>(new Set());
  const [sending, setSending] = useState(false);
  const [copied, setCopied] = useState(false);
  const [scrolled, onScroll] = useScrolled();
  const rows = useRef<HTMLUListElement>(null);
  const toast = useToast();

  const list = view === "projects" ? projects : toolchains;
  const all = [...toolchains, ...projects];
  const focused = all.find((entry) => entry.id === focus) ?? list[0];
  const scanned = props.developerGb !== null;
  const selectable = (entry: DevEntry) => (entry.risk === "low" || entry.risk === "check") && entry.reviewIds.length > 0 && !inReview(entry, sent);
  const picked = all.filter((entry) => selectable(entry) && selection.has(entry.id));
  const pickedGb = sum(picked, (entry) => entry.gb);
  const has = picked.length > 0;
  const sentCount = all.filter((entry) => inReview(entry, sent)).length;

  const toggle = (entry: DevEntry) => {
    if (!selectable(entry)) {
      return;
    }
    const next = new Set(selection);
    if (next.has(entry.id)) {
      next.delete(entry.id);
    } else {
      next.add(entry.id);
    }
    setSelection(next);
    setFocus(entry.id);
  };

  const send = () => {
    setSending(true);
    void props.onSendToReview(picked).then((message) => {
      setSending(false);
      setSelection(new Set());
      toast.show(message);
    });
  };

  return (
    <>
      <PageHeader
        title="Developer"
        headingId={headingIdFor("developer")}
        subtitle="Toolchains, caches and build output. Most of it rebuilds itself; nothing moves until you approve it in Cleanup."
        actions={
          <>
            <LinkButton href={hrefFor("review")} size="sm">Open review</LinkButton>
            <Button
              variant="soft"
              size="sm"
              disabled={status.running}
              onClick={() => {
                void props.onAddFolder().then((message) => message && toast.show(message));
              }}
            >
              Add folder…
            </Button>
            {status.running ? (
              <Button variant="secondary" size="sm" onClick={props.onCancel}>
                <Spinner size={10} /> Cancel
              </Button>
            ) : (
              <Button variant="soft" size="sm" onClick={props.onScan}>{scanned ? "Scan again" : "Scan"}</Button>
            )}
          </>
        }
      />
      <div className={`dev-body${scanned ? " dev-body-above-bar" : ""}`}>
        {(status.text || status.failed || status.note) && (
          <span role="status" className="v2-card-note dev-status">{status.failed ?? status.text ?? status.note}</span>
        )}
        <section aria-label="Developer summary" className="v2-grid-4">
          <StatCard tone="indigo" icon="code" label="Developer files" value={props.developerGb === null ? "—" : size({ gb: props.developerGb, lowerBound: props.developerLowerBound })} sub={props.shareOfUsed ? `${props.shareOfUsed} of used space` : "Not scanned yet"} />
          <StatCard tone="green" icon="refresh" label="Rebuilds itself" value={props.rebuildsGb === null ? "—" : formatGb(props.rebuildsGb)} sub="Low risk" />
          <StatCard tone="orange" icon="folder" label="Project build folders" value={props.projectsGb === null ? "—" : size({ gb: props.projectsGb, lowerBound: props.projectsLowerBound })} sub={props.projectsNote} />
          <StatCard tone="teal" icon="package" label="Docker" value={dockerValue(props.docker)} sub={dockerSub(props.docker)} />
        </section>

        {!scanned ? (
          <section aria-label={status.running ? "Scanning" : "Not scanned yet"} className="v2-card dev-empty">
            {status.running ? <Spinner size={34} /> : <IconTile tone="indigo" icon="code" size={42} glow />}
            <span className="v2-row-title">{status.running ? "Scanning…" : "Not scanned yet"}</span>
            <span className="v2-card-note">
              Scanning measures known developer caches and finds build folders in your projects, in your home folder and any folder you add. It only reads sizes. macOS may ask before Kleari can read some of your folders.
            </span>
            {!status.running && <Button variant="primary" size="sm" onClick={props.onScan}>Scan</Button>}
          </section>
        ) : (
          <div className="dev-grid">
            <section aria-labelledby="dev-list" className="v2-card dev-list">
              <div className="dev-list-head">
                <h2 id="dev-list" className="v2-card-title-sm">{view === "projects" ? "Project build folders" : "Toolchains and caches"}</h2>
                <Segmented
                  label="View"
                  value={view}
                  onChange={(next) => {
                    setView(next);
                    setFocus(null);
                    rows.current?.scrollTo({ top: 0 });
                  }}
                  options={[{ value: "toolchains", label: "Toolchains" }, { value: "projects", label: "Projects" }]}
                />
              </div>
              <span className="v2-card-note dev-list-note">
                {view === "projects"
                  ? "Only folders a build or install makes again (node_modules, target, build). Your source code is never selected."
                  : "Found by a read-only scan of the usual developer folders. Pick a row to see what it holds."}
              </span>
              {list.length === 0 ? (
                <span className="v2-card-note dev-list-note">{view === "projects" ? "No project build folders were found." : "No developer caches were found."}</span>
              ) : (
                <ul ref={rows} tabIndex={0} aria-label={view === "projects" ? "Project build folder list" : "Toolchain and cache list"} className="dev-rows" onScroll={onScroll}>
                  <ScrollEdgeGlass visible={scrolled} inset={0} as="li" />
                  {list.map((entry) => {
                    const can = selectable(entry);
                    const on = can && selection.has(entry.id);
                    const pill = inReview(entry, sent) ? { label: "In review", status: "link" as const } : riskPill[entry.risk];
                    return (
                      <li key={entry.id} className={focused?.id === entry.id ? "focused" : ""}>
                        <input type="checkbox" aria-label={`Select ${entry.name}`} checked={on} disabled={!can} onChange={() => toggle(entry)} style={{ opacity: can ? 1 : 0.4 }} />
                        <button type="button" className="dev-row-main" aria-pressed={focused?.id === entry.id} onClick={() => setFocus(entry.id)}>
                          <IconTile tone={entry.tone} icon={entry.icon} size={30} />
                          <span className="v2-row-text">
                            <span className="v2-row-title">{entry.name}</span>
                            <span className="v2-row-sub v2-ellipsis">{entry.sub}</span>
                          </span>
                        </button>
                        <span className="dev-size">{size(entry)}</span>
                        <span className="dev-pill"><Pill status={pill.status}>{pill.label}</Pill></span>
                      </li>
                    );
                  })}
                </ul>
              )}
            </section>

            <div className="dev-side">
              {focused && (
                <section aria-labelledby="dev-focus" className="v2-card dev-detail">
                  <div className="dev-detail-head">
                    <IconTile tone={focused.tone} icon={focused.icon} size={42} glow />
                    <span className="v2-row-text">
                      <h2 id="dev-focus" className="v2-card-title-sm v2-ellipsis">{focused.name}</h2>
                      <span className="v2-muted v2-num dev-detail-meta">{size(focused)} · {focused.meta}</span>
                    </span>
                  </div>
                  <div className="dev-detail-text">
                    <span><strong>What it is.</strong> {focused.what}</span>
                    <span><strong>If it goes.</strong> {focused.after}</span>
                  </div>
                  {focused.parts.length > 0 && (
                    <ul className="v2-info-list dev-parts">
                      {focused.parts.map((part) => (
                        <li key={part.name}><span className="v2-ellipsis">{part.name}</span><strong className="v2-num">{formatGb(part.gb)}</strong></li>
                      ))}
                    </ul>
                  )}
                  <span className="v2-mono v2-ellipsis dev-path">{focused.path}</span>
                </section>
              )}

              <section aria-labelledby="dev-docker" className="v2-card dev-docker">
                <div className="dev-docker-head">
                  <IconTile tone="teal" icon="package" size={34} />
                  <h2 id="dev-docker" className="v2-card-title-sm">Docker</h2>
                  <Pill status="muted">Read-only</Pill>
                </div>
                {props.docker?.status === "running" ? (
                  <>
                    <div className="dev-docker-bar" aria-hidden="true">
                      {props.docker.parts.map((part) => (
                        <span key={part.label} className={`tone-${part.tone}`} style={{ width: `${(props.docker?.status === "running" && props.docker.totalGb > 0 ? (part.gb / props.docker.totalGb) * 100 : 0).toFixed(1)}%` }} />
                      ))}
                    </div>
                    <div className="dev-docker-legend">
                      {props.docker.parts.map((part) => (
                        <span key={part.label} className={`tone-${part.tone}`}>
                          <span className="dev-docker-dot" aria-hidden="true" />
                          <span className="v2-ellipsis">{part.label}</span>
                          <strong className="v2-num">{formatGb(part.gb)}</strong>
                        </span>
                      ))}
                    </div>
                    {!props.docker.complete && <span className="v2-card-note">Docker didn’t report every size, so these are minimums.</span>}
                  </>
                ) : (
                  <span className="v2-card-note">
                    {props.docker?.status === "notRunning"
                      ? `Docker isn’t running.${props.docker.imageGb === null ? "" : ` Its disk image takes ${formatGb(props.docker.imageGb)}.`} Start Docker and scan again to see what’s inside.`
                      : props.docker?.status === "notInstalled"
                        ? "Docker Desktop wasn’t found on this Mac."
                        : props.docker?.status === "notChecked"
                          ? "Docker wasn’t checked because the scan was stopped."
                          : "Scan to see Docker’s disk use."}
                  </span>
                )}
                {props.docker && props.docker.status !== "notInstalled" && props.docker.status !== "notChecked" && (
                  <>
                    <span className="v2-card-note dev-docker-note">Kleari never changes Docker data. To reclaim space yourself, run this in Terminal:</span>
                    <div className="dev-command">
                      <code>{DOCKER_COMMAND}</code>
                      <Button
                        variant="soft"
                        size="sm"
                        className="dev-copy"
                        onClick={() => {
                          void navigator.clipboard.writeText(DOCKER_COMMAND).then(
                            () => {
                              setCopied(true);
                              setTimeout(() => setCopied(false), 2000);
                            },
                            () => toast.show("Couldn’t copy the command", "warn"),
                          );
                        }}
                      >
                        {copied ? "Copied" : "Copy"}
                      </Button>
                    </div>
                  </>
                )}
              </section>
            </div>
          </div>
        )}
      </div>

      {scanned && (
        <FloatingBar>
          <div className="v2-float-text">
            <span role="status" className="v2-float-title">
              {has ? `${picked.length} selected · ${formatGb(pickedGb)}` : sentCount ? `${plural(sentCount, "item")} in review` : "Nothing selected"}
            </span>
            <span className="v2-float-sub">Sending to review adds these to Cleanup step 2. Nothing moves until you approve it there.</span>
          </div>
          <div className="v2-float-actions">
            <Button
              variant="secondary"
              className="dev-select-low"
              onClick={() => setSelection(new Set([...selection, ...list.filter((entry) => entry.risk === "low" && selectable(entry)).map((entry) => entry.id)]))}
            >
              Select low risk
            </Button>
            <Button variant="primary" disabled={!has || sending} onClick={send}>
              {has ? `Send ${picked.length} to review` : "Send to review"}
            </Button>
          </div>
        </FloatingBar>
      )}

      <Toast message={toast.message} bottom={92}>
        <a className="v2-link dev-toast-link" href={hrefFor("review")}>Open review</a>
      </Toast>
    </>
  );
}

function dockerValue(docker: DockerView | null): string {
  if (!docker) {
    return "—";
  }
  if (docker.status === "running") {
    return size({ gb: docker.totalGb, lowerBound: !docker.complete });
  }
  return docker.status === "notRunning" && docker.imageGb !== null ? formatGb(docker.imageGb) : "—";
}

function dockerSub(docker: DockerView | null): string {
  if (!docker) {
    return "Not scanned yet";
  }
  if (docker.status === "running") {
    return "Read-only";
  }
  if (docker.status === "notChecked") {
    return "Not checked";
  }
  return docker.status === "notRunning" ? "Not running · disk image" : "Not installed";
}
