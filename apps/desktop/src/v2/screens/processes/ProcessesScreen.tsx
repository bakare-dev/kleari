import { useLayoutEffect, useRef, useState } from "react";
import { Glyph, type IconName } from "../../ui/icons";
import {
  Button,
  chartSegments,
  IconTile,
  LiveBadge,
  Modal,
  PageHeader,
  Pill,
  SearchField,
  Segmented,
  StatCard,
  Toast,
  useScrolled,
  ScrollEdgeGlass,
  useToast,
  type LiveState,
  type Status,
  type Tone,
} from "../../ui/components";
import { formatGb, plural } from "../../ui/format";
import { headingIdFor } from "../../shell/routes";
import "./processes.css";

export type ProcessKind = "app" | "background" | "system";

export interface ProcessSample {
  id: string;
  name: string;
  /** "24 processes" for an app; where it runs from otherwise. */
  sub: string;
  kind: ProcessKind;
  /** Current CPU percent (share of one core); `null` before a reading. */
  cpu: number | null;
  /** Some helper processes have no reading yet, so `cpu` is a minimum. */
  cpuLowerBound: boolean;
  /** CPU readings taken while the page is open, oldest first; `null` is a gap. */
  cpuHistory: readonly (number | null)[];
  memGb: number;
  pid: number;
  processCount: number;
  user: string;
  started: string;
  threads: string;
  path: string;
  tone: Tone;
  /** Kleari itself: never quit from here. */
  isSelf: boolean;
  /** Only regular apps of this account can be asked to quit. */
  canQuit: boolean;
}

/** What happened when an app was asked to quit. */
export type QuitResult = "requested" | "notRunning" | "notAllowed" | "declined" | "unavailable";

interface ProcessesScreenProps {
  processes: readonly ProcessSample[];
  /** Processes running in total, including those not listed. */
  runningTotal: number;
  /** Processes macOS doesn't let Kleari read (other accounts'). */
  unreadableCount: number;
  cores: number;
  /** All CPUs, 0–100; `null` before a reading. */
  totalCpu: number | null;
  memoryUsedGb: number | null;
  memoryTotalGb: number | null;
  live: LiveState;
  /** Why the readings are waiting, stale or unavailable. */
  liveNote: string | null;
  historySlots: number;
  paused: boolean;
  onTogglePause: () => void;
  /** Asks the app to quit, as choosing Quit from its menu does. Always confirmed first. */
  onQuit: (process: ProcessSample) => Promise<QuitResult>;
}

type Sort = "cpu" | "mem";

const kindLabel: Record<ProcessKind, string> = { app: "App", background: "Background process", system: "System process" };

const quitMessages: Record<QuitResult, (name: string) => string> = {
  requested: (name) => `Asked ${name} to quit. It may ask you to save first.`,
  notRunning: (name) => `${name} isn’t running any more, so nothing was sent.`,
  notAllowed: (name) => `Kleari can’t ask ${name} to quit.`,
  declined: (name) => `macOS didn’t deliver the request to ${name}. Quit it from its own menu.`,
  unavailable: (name) => `Kleari couldn’t ask ${name} to quit. Nothing was changed.`,
};

function statusOf(process: ProcessSample): { label: string; status: Status } {
  if (process.isSelf) {
    return { label: "This app", status: "link" };
  }
  return process.kind === "system" ? { label: "Protected", status: "muted" } : { label: "Running", status: "ok" };
}

/** Only another regular app of this account can be asked to quit. */
export function canQuit(process: ProcessSample): boolean {
  return process.canQuit && process.kind === "app" && !process.isSelf;
}

export function cpuText(process: ProcessSample): string {
  if (process.cpu === null) {
    return "—";
  }
  return `${process.cpuLowerBound ? "≥ " : ""}${process.cpu.toFixed(1)}%`;
}

export function ProcessesScreen(props: ProcessesScreenProps) {
  const { processes, runningTotal, cores, paused, onTogglePause, onQuit } = props;
  const [kind, setKind] = useState<ProcessKind | "all">("all");
  const [sort, setSort] = useState<Sort>("cpu");
  const [query, setQuery] = useState("");
  const [openId, setOpenId] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [scrolled, onScroll] = useScrolled();
  const toast = useToast();

  // Live readings re-sort the list. While the pointer or keyboard focus is in it, the order is
  // held, so a row never moves out from under a click.
  const [holding, setHolding] = useState(false);
  const heldOrder = useRef<readonly string[]>([]);

  const needle = query.trim().toLowerCase();
  const filtered = processes.filter(
    (process) => (kind === "all" || process.kind === kind) && (needle === "" || `${process.name} ${process.sub}`.toLowerCase().includes(needle)),
  );
  const sorted = [...filtered].sort((a, b) => (sort === "mem" ? b.memGb - a.memGb : (b.cpu ?? -1) - (a.cpu ?? -1) || b.memGb - a.memGb));
  const position = (id: string) => {
    const index = heldOrder.current.indexOf(id);
    return index === -1 ? Number.MAX_SAFE_INTEGER : index;
  };
  const shown = holding ? [...filtered].sort((a, b) => position(a.id) - position(b.id)) : sorted;
  // Record the order that was actually committed, so a hold freezes what's on screen. Layout
  // effects run before the browser can deliver the next pointer or focus event.
  const committedOrder = shown.map((process) => process.id).join("\n");
  useLayoutEffect(() => {
    if (!holding) {
      heldOrder.current = committedOrder.split("\n");
    }
  }, [holding, committedOrder]);
  const hold = (on: boolean) => setHolding(on);
  const open = processes.find((process) => process.id === openId);
  const busiest = processes.reduce<ProcessSample | undefined>((top, process) => ((process.cpu ?? -1) > (top?.cpu ?? -1) ? process : top), undefined);
  const count = (value: ProcessKind | "all") => processes.filter((process) => value === "all" || process.kind === value).length;

  const close = () => {
    setOpenId(null);
    setConfirming(false);
  };

  return (
    <>
      <PageHeader
        title="Processes"
        headingId={headingIdFor("processes")}
        subtitle="What is running right now. Quitting an app always asks first, and system processes are protected."
        actions={
          <>
            <LiveBadge state={props.live} />
            <Button variant="soft" size="sm" onClick={onTogglePause}>{paused ? "Resume" : "Pause"}</Button>
          </>
        }
      />
      <div className="proc-body">
        {props.liveNote && <span role="status" className="v2-card-note">{props.liveNote}</span>}
        <section aria-label="Totals" className="v2-grid-4">
          <StatCard tone="indigo" icon="pulse" label="CPU" value={props.totalCpu === null ? "—" : `${props.totalCpu.toFixed(0)}%`} sub={`Across ${cores} cores`} />
          <StatCard
            tone="green"
            icon="chip"
            label="Memory"
            value={props.memoryUsedGb === null ? "—" : `${props.memoryUsedGb.toFixed(1)} GB`}
            sub={props.memoryTotalGb === null ? "" : `of ${props.memoryTotalGb.toFixed(0)} GB`}
          />
          <StatCard tone="blue" icon="list" label="Running" value={String(runningTotal)} sub="processes" />
          <StatCard tone="orange" icon="bolt" label="Busiest" value={busiest && busiest.cpu !== null ? busiest.name : "—"} sub={busiest && busiest.cpu !== null ? `${cpuText(busiest)} CPU` : "Waiting for a reading"} />
        </section>

        <section aria-labelledby="proc-list" className="v2-card proc-card">
          <h2 id="proc-list" className="v2-visually-hidden">Running processes</h2>
          <div className="proc-toolbar">
            <Segmented
              label="Show"
              value={kind}
              onChange={setKind}
              options={[
                { value: "all", label: `All ${count("all")}` },
                { value: "app", label: `Apps ${count("app")}` },
                { value: "background", label: `Background ${count("background")}` },
                { value: "system", label: `System ${count("system")}` },
              ]}
            />
            <SearchField label="Search processes" value={query} onChange={setQuery} strong />
          </div>
          <div className="proc-head proc-grid">
            <span>Name</span>
            {(["cpu", "mem"] as const).map((column) => (
              <button key={column} type="button" className={`proc-sort${sort === column ? " on" : ""}`} aria-label={`Sort by ${column === "cpu" ? "CPU" : "Memory"}`} onClick={() => setSort(column)}>
                {column === "cpu" ? "CPU" : "Memory"}
                <span aria-hidden="true">{sort === column ? "↓" : ""}</span>
              </button>
            ))}
            <span className="proc-right">Status</span>
          </div>
          <ul
            tabIndex={0}
            aria-label="Process list"
            className="proc-list"
            onScroll={onScroll}
            onPointerEnter={() => hold(true)}
            onPointerLeave={() => hold(false)}
            onFocus={() => hold(true)}
            onBlur={(event) => {
              if (!event.currentTarget.contains(event.relatedTarget)) {
                hold(false);
              }
            }}
          >
            <ScrollEdgeGlass visible={scrolled} inset={0} as="li" />
            {shown.map((process) => {
              const status = statusOf(process);
              return (
                <li key={process.id}>
                  <button
                    type="button"
                    className={`proc-row proc-grid${openId === process.id ? " open" : ""}`}
                    aria-label={`${process.name}, CPU ${process.cpu === null ? "not measured yet" : `${process.cpuLowerBound ? "at least " : ""}${process.cpu.toFixed(1)} percent`}, memory ${formatGb(process.memGb)}`}
                    onClick={() => setOpenId(process.id)}
                  >
                    <span className="proc-name">
                      <IconTile tone={process.tone} initial={process.name.charAt(0).toUpperCase()} size={28} />
                      <span className="v2-row-text">
                        <span className="v2-row-title">{process.name}</span>
                        <span className="v2-row-sub v2-ellipsis">{process.sub}</span>
                      </span>
                    </span>
                    <span className="proc-cpu">
                      <span className="proc-cpu-track" aria-hidden="true">
                        <span style={{ width: `${Math.min(100, ((process.cpu ?? 0) / 20) * 100).toFixed(0)}%` }} />
                      </span>
                      <span className="proc-cpu-value">{cpuText(process)}</span>
                    </span>
                    <span className="proc-right v2-num">{formatGb(process.memGb)}</span>
                    <span className="proc-right"><Pill status={status.status}>{status.label}</Pill></span>
                  </button>
                </li>
              );
            })}
          </ul>
          {shown.length === 0 && <span className="proc-empty">{processes.length === 0 ? "No readings yet." : `No processes match “${query}”.`}</span>}
          <span className="proc-foot">
            Showing {processes.length} of {runningTotal} processes: the busiest and the largest.
            {props.unreadableCount > 0 && ` ${plural(props.unreadableCount, "process", "processes")} reported no memory value, so ${props.unreadableCount === 1 ? "it isn’t" : "they aren’t"} listed.`}
          </span>
        </section>
      </div>

      <Toast message={toast.message} />

      <Modal open={open !== undefined} onClose={close} labelledBy="proc-sheet-title" variant="panel">
        {open && <ProcessPanel process={open} slots={props.historySlots} onClose={close} onQuit={() => setConfirming(true)} />}
      </Modal>

      <Modal open={open !== undefined && confirming} onClose={() => setConfirming(false)} labelledBy="proc-quit-title" role="alertdialog" className="proc-quit">
        {open && (
          <div className="v2-dialog-body v2-dialog-centered">
            <IconTile tone={open.tone} initial={open.name.charAt(0).toUpperCase()} size={54} glow />
            <h2 id="proc-quit-title" className="v2-dialog-title">{`Quit ${open.name}?`}</h2>
            <p className="v2-dialog-text">{`${open.name} will be asked to quit, the same as choosing Quit from its menu. It may ask you to save first, or decline.`}</p>
            <div className="v2-dialog-actions">
              <Button variant="secondary" onClick={() => setConfirming(false)}>Cancel</Button>
              <Button
                variant="primary"
                onClick={() => {
                  const target = open;
                  close();
                  void onQuit(target).then((result) => toast.show(quitMessages[result](target.name)));
                }}
              >
                Quit
              </Button>
            </div>
          </div>
        )}
      </Modal>
    </>
  );
}

interface ProcessPanelProps {
  process: ProcessSample;
  slots: number;
  onClose: () => void;
  onQuit: () => void;
}

function protectedText(process: ProcessSample): string {
  if (process.isSelf) {
    return "This is Kleari itself. Quit it from its own menu.";
  }
  if (process.kind === "system") {
    return "A macOS system process. Kleari never stops these.";
  }
  if (process.kind === "app") {
    return "An app that ships with macOS, or one Kleari can’t identify for certain. Quit it from its own menu.";
  }
  return "Kleari only asks regular apps to quit. Background processes are shown, not stopped.";
}

function ProcessPanel({ process, slots, onClose, onQuit }: ProcessPanelProps) {
  const status = statusOf(process);
  const current = cpuText(process);
  const max = Math.max(25, ...process.cpuHistory.map((value) => value ?? 0));
  const segments = chartSegments(process.cpuHistory, slots, max, { width: 300, bottom: 52, span: 44 });
  const facts: { label: string; value: string; tone: Tone; icon: IconName }[] = [
    { label: "User", value: process.user, tone: "blue", icon: "user" },
    { label: "Started", value: process.started, tone: "orange", icon: "clock" },
    { label: "Threads", value: process.threads, tone: "purple", icon: "list" },
    { label: "Location", value: process.path, tone: "teal", icon: "folder" },
  ];
  const tiles: { label: string; value: string; tone: Tone; icon: IconName }[] = [
    { label: "CPU", value: current, tone: "indigo", icon: "pulse" },
    { label: "Memory", value: formatGb(process.memGb), tone: "green", icon: "chip" },
    { label: "Processes", value: String(process.processCount), tone: "orange", icon: "layers" },
  ];
  return (
    <div className="proc-panel">
      <div className={`proc-hero tone-${process.tone}`}>
        <span className="proc-hero-glow" aria-hidden="true" />
        <span className="proc-hero-blob" aria-hidden="true" />
        <div className="proc-hero-row">
          <span className="proc-hero-initial" aria-hidden="true">{process.name.charAt(0).toUpperCase()}</span>
          <span className="v2-row-text">
            <h2 id="proc-sheet-title" className="v2-ellipsis">{process.name}</h2>
            <span className="proc-hero-sub">{kindLabel[process.kind]} · PID {process.pid}</span>
          </span>
          <button type="button" className="proc-hero-close" aria-label="Close" onClick={onClose}><Glyph name="close" size={12} strokeWidth={2.6} /></button>
        </div>
        <span className="proc-hero-status">
          <span className={`proc-hero-dot ${process.kind === "system" ? "sys" : "ok"}`} aria-hidden="true" />
          {status.label}
        </span>
      </div>
      <div className="proc-tiles">
        {tiles.map((tile) => (
          <span key={tile.label} className={`proc-tile tone-${tile.tone}`}>
            <IconTile tone={tile.tone} icon={tile.icon} size={22} glow />
            <span className="v2-muted proc-tile-label">{tile.label}</span>
            <span className="proc-tile-value">{tile.value}</span>
          </span>
        ))}
      </div>
      <div className="proc-spark">
        <div className="proc-spark-head"><strong>CPU · while this page is open</strong><span>{process.cpu === null ? "Waiting" : `${current} now`}</span></div>
        <svg role="img" aria-label="CPU use while this page is open" width="100%" height="54" viewBox="0 0 300 54" preserveAspectRatio="none">
          <defs>
            <linearGradient id="proc-spark-area" x1="0" y1="0" x2="0" y2="1">
              <stop offset="0" stopColor="#8B5CF6" stopOpacity="0.35" />
              <stop offset="1" stopColor="#EC4899" stopOpacity="0" />
            </linearGradient>
            <linearGradient id="proc-spark-line" x1="0" y1="0" x2="1" y2="0">
              <stop offset="0" stopColor="#3B82F6" />
              <stop offset="0.5" stopColor="#8B5CF6" />
              <stop offset="1" stopColor="#EC4899" />
            </linearGradient>
          </defs>
          {segments.map((segment) => (
            <g key={segment.firstX}>
              <polygon points={`${segment.firstX},56 ${segment.points} ${segment.lastX},56`} fill="url(#proc-spark-area)" />
              <polyline points={segment.points} fill="none" stroke="url(#proc-spark-line)" strokeWidth="2.4" vectorEffect="non-scaling-stroke" strokeLinejoin="round" strokeLinecap="round" />
            </g>
          ))}
        </svg>
        {segments.length === 0 && <span className="proc-spark-empty">Readings appear here while this page is open.</span>}
      </div>
      <ul className="proc-facts">
        {facts.map((fact) => (
          <li key={fact.label}>
            <IconTile tone={fact.tone} icon={fact.icon} size={20} />
            <span className="v2-muted">{fact.label}</span>
            <span className="proc-fact-value">{fact.value}</span>
          </li>
        ))}
      </ul>
      <div className="proc-panel-spacer" />
      {canQuit(process) ? (
        <Button variant="primary" className="proc-quit-btn" onClick={onQuit}>
          {`Quit ${process.name}…`}
        </Button>
      ) : (
        <div className="v2-info-strip proc-protected">
          <span className="proc-shield"><Glyph name="shield" size={16} /></span>
          <span>{protectedText(process)}</span>
        </div>
      )}
    </div>
  );
}
