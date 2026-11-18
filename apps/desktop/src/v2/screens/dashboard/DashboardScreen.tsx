import { useDeferredValue, useEffect, useRef, type FormEvent, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import type { SearchResult, SearchTarget } from "../../../features/dashboard/search";
import { Glyph, type IconName } from "../../ui/icons";
import { Button, Dot, IconTile, LinkButton, ScrollBody, seriesPoints, type Tone } from "../../ui/components";
import { formatGb, plural } from "../../ui/format";
import { headingIdFor, hrefFor, navigate, type V2Route } from "../../shell/routes";
import "./dashboard.css";

export interface DashboardMetric {
  label: string;
  tone: Tone;
  icon: IconName;
  value: string;
  sub: string;
  /** 0–100 for a meter; omitted for the CPU sparkline card. */
  percent?: number;
  history?: readonly number[];
  historyLabel?: string;
}

export interface StorageCategory {
  label: string;
  gb: number;
  tone: Tone;
}

export interface DashboardFinding {
  title: string;
  sub: string;
  gb: number;
  tone: Tone;
  icon: IconName;
}

export interface DashboardData {
  /** When the readings were taken; `null` before the first one. */
  sampledAt: string | null;
  /** Why readings are missing or old, if they are. */
  note?: string | null;
  metrics: readonly DashboardMetric[];
  storage: {
    volume: string;
    /** When the categories were measured; `null` when nothing has been analyzed yet. */
    lastScan: string | null;
    usedGb: number;
    totalGb: number;
    categories: readonly StorageCategory[];
    /** An analysis is running now. */
    analyzing?: boolean;
  };
  quarantineCount: number;
  /** Shown under Quarantine in Quick actions, such as the reminder size being passed. */
  quarantineNote?: string;
  findings: readonly DashboardFinding[];
  /** Said when there are no findings to show. */
  findingsEmpty?: string;
}

interface DashboardScreenProps {
  data: DashboardData;
  /** The search, kept by the app so results are still there after opening one. */
  query: string;
  /** An empty query clears the search, and the list searches it filled in. */
  onQueryChange: (query: string) => void;
  /** Searches what Kleari holds this session; called only while there is a query. */
  search: (query: string) => SearchResult;
  /** Before a result's page opens: fills that page's list search with the query. */
  onOpen: (target: SearchTarget, query: string) => void;
  /** Starts a read-only Storage analysis; absent where analysis isn't available. */
  onAnalyze?: () => void;
  /** Issue alerts, shown under the readings when there are any. */
  alerts?: ReactNode;
}

function MetricCard({ metric }: { metric: DashboardMetric }) {
  return (
    <div className={`v2-card dash-metric tone-${metric.tone}`}>
      <div className="dash-metric-top">
        <IconTile tone={metric.tone} icon={metric.icon} size={42} glow />
        <div className="dash-metric-text">
          <span className="dash-metric-label">{metric.label}</span>
          <span className="dash-metric-value">{metric.value}</span>
          <span className="dash-metric-sub" title={metric.sub}>{metric.sub}</span>
        </div>
      </div>
      {metric.percent !== undefined && (
        <div className="dash-meter">
          <div className="dash-meter-track" aria-hidden="true">
            <div className="dash-meter-fill" style={{ width: `${metric.percent}%` }} />
          </div>
          <span className="v2-num">{metric.percent}%</span>
        </div>
      )}
      {metric.history !== undefined && <Sparkline values={metric.history} label={metric.historyLabel ?? metric.label} />}
    </div>
  );
}

function Sparkline({ values, label }: { values: readonly number[]; label: string }) {
  if (values.length < 2) {
    return <span className="dash-spark-empty">{label}</span>;
  }
  const max = Math.max(...values, 1) * 1.15;
  const line = seriesPoints(values, 160, 30, max, 2, 2);
  return (
    <svg className="dash-spark" role="img" aria-label={label} width="100%" height="30" viewBox="0 0 160 30" preserveAspectRatio="none">
      <defs>
        <linearGradient id="dash-spark-fill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" className="dash-spark-stop" stopOpacity="0.35" />
          <stop offset="1" className="dash-spark-stop" stopOpacity="0" />
        </linearGradient>
      </defs>
      <polygon points={`0,30 ${line} 160,30`} fill="url(#dash-spark-fill)" />
      <polyline points={line} fill="none" className="dash-spark-line" strokeWidth="2" strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

function StorageDonut({ usedGb, totalGb, categories }: DashboardData["storage"]) {
  const radius = 60;
  const circumference = 2 * Math.PI * radius;
  const gap = 1.5;
  let offset = 0;
  const label = `Storage used: ${Math.round(usedGb)} GB of ${Math.round(totalGb)} GB, by category`;
  return (
    <div className="dash-donut">
      <svg role="img" aria-label={label} width="156" height="156" viewBox="0 0 156 156">
        <g transform="rotate(-90 78 78)">
          <circle cx="78" cy="78" r={radius} fill="none" className="v2-track-stroke" strokeWidth="20" />
          {categories.map((category) => {
            const length = (category.gb / usedGb) * circumference;
            const dash = `${Math.max(length - gap, 0).toFixed(2)} ${(circumference - length + gap).toFixed(2)}`;
            const segment = (
              <circle
                key={category.label}
                className={`dash-donut-seg tone-${category.tone}`}
                cx="78"
                cy="78"
                r={radius}
                fill="none"
                strokeWidth="20"
                strokeDasharray={dash}
                strokeDashoffset={(-offset).toFixed(2)}
              />
            );
            offset += length;
            return segment;
          })}
        </g>
      </svg>
      <div className="dash-donut-center" aria-hidden="true">
        <span>{Math.round(usedGb)} GB</span>
        <span>used of {Math.round(totalGb)} GB</span>
      </div>
    </div>
  );
}

interface QuickAction {
  title: string;
  sub: string;
  route: V2Route;
  tone: Tone;
  icon: IconName;
}

function SearchResults({ result, query, onOpen, onClear }: { result: SearchResult; query: string; onOpen: (target: SearchTarget) => void; onClear: () => void }) {
  const link = (target: SearchTarget) => ({ href: hrefFor(target.route, target.detail), onClick: () => onOpen(target) });
  return (
    <section aria-label="Search results" className="dash-results">
      <div className="dash-results-head" role="status">
        <span>{result.total === 0 ? `Nothing matches “${query}”.` : `${plural(result.total, "match", "matches")} for “${query}”`}</span>
        <Button variant="soft" size="sm" onClick={onClear}>Clear search</Button>
      </div>
      {result.groups.map((group) => (
        <section key={group.source.id} aria-labelledby={`dash-found-${group.source.id}`} className="v2-card dash-list dash-found">
          <div className="dash-card-head dash-list-title">
            <h2 id={`dash-found-${group.source.id}`} className="v2-card-title">
              {group.source.label} <span className="v2-muted dash-found-count">{group.total}</span>
            </h2>
            {group.total > group.matches.length && <a className="v2-link" {...link(group.showAll)}>Show all {group.total}</a>}
          </div>
          {group.matches.map((item) => (
            <a key={item.id} className="v2-row-action" {...link(item.target ?? group.source.target)}>
              <IconTile tone={item.tone ?? group.source.tone} icon={item.icon ?? group.source.icon} size={30} />
              <span className="v2-row-text">
                <span className="v2-row-title v2-ellipsis">{item.title}</span>
                <span className="dash-row-sub v2-ellipsis">{item.sub}</span>
              </span>
              <span className="v2-chevron"><Glyph name="chevronRight" size={14} /></span>
            </a>
          ))}
        </section>
      ))}
      {result.pending.length > 0 && (
        <section aria-label="Not searched yet" className="v2-card dash-list dash-pending">
          <h2 className="v2-card-title-sm dash-list-title">Not searched yet</h2>
          {result.pending.map((source) => (
            <a key={source.id} className="v2-row-action" href={hrefFor(source.target.route, source.target.detail)}>
              <IconTile tone={source.tone} icon={source.icon} size={28} />
              <span className="v2-row-text">
                <span className="v2-row-title">{source.label}</span>
                <span className="dash-row-sub">{source.pending}</span>
              </span>
              <span className="v2-chevron"><Glyph name="chevronRight" size={14} /></span>
            </a>
          ))}
        </section>
      )}
    </section>
  );
}

export function DashboardScreen({ data, query, onQueryChange, search: searchFor, onOpen, onAnalyze, alerts }: DashboardScreenProps) {
  const search = useRef<HTMLInputElement>(null);
  // Typing stays quick on big lists: the results catch up after each key.
  const deferred = useDeferredValue(query);
  const result = deferred.trim() === "" ? null : searchFor(deferred);
  const open = (target: SearchTarget) => onOpen(target, deferred.trim());

  // ⌘K focuses search, as the field's hint says.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key.toLowerCase() === "k" && event.metaKey && !event.ctrlKey && !event.altKey) {
        event.preventDefault();
        search.current?.focus();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Return opens the first result.
  function submit(event: FormEvent) {
    event.preventDefault();
    const first = result?.groups[0];
    if (first) {
      const target = first.matches[0].target ?? first.source.target;
      open(target);
      navigate(target.route, target.detail);
    }
  }

  // Escape clears the search, as the field's own clear button does.
  function onFieldKey(event: ReactKeyboardEvent<HTMLInputElement>) {
    if (event.key === "Escape" && query !== "") {
      event.preventDefault();
      onQueryChange("");
    }
  }

  const actions: QuickAction[] = [
    { title: "Scan for caches and junk", sub: "Read-only scan of folders you allow", route: "cleanup", tone: "blue", icon: "search" },
    { title: "Review large files", sub: "Sort and inspect. Nothing moves.", route: "storage", tone: "green", icon: "file" },
    { title: "Developer files", sub: "Xcode, simulators, node_modules", route: "developer", tone: "indigo", icon: "code" },
    {
      title: "Quarantine",
      sub: data.quarantineNote ?? (data.quarantineCount === 0 ? "Nothing held" : `${data.quarantineCount} ${data.quarantineCount === 1 ? "item" : "items"} you can restore`),
      route: "quarantine",
      tone: "orange",
      icon: "quarantine",
    },
  ];
  const { storage } = data;

  return (
    <>
      <header className="v2-page-header dash-header">
        <div className="v2-page-header-row dash-header-row">
          <div className="v2-page-title">
            <h1 id={headingIdFor("dashboard")} tabIndex={-1}>Dashboard</h1>
            <span className="v2-page-subtitle">{data.sampledAt ? `Live readings, sampled ${data.sampledAt}` : "Waiting for the first reading"}</span>
          </div>
          <form role="search" onSubmit={submit}>
            <label className="v2-search dash-search">
              <Glyph name="search" size={14} />
              <input
                ref={search}
                type="search"
                aria-label="Search Kleari: pages, findings, apps, files and more"
                placeholder="Search everything"
                value={query}
                onChange={(event) => onQueryChange(event.target.value)}
                onKeyDown={onFieldKey}
              />
              <kbd>⌘K</kbd>
            </label>
          </form>
        </div>
      </header>

      <ScrollBody label="Dashboard" className="dash-body">
        {result ? (
          <SearchResults result={result} query={deferred.trim()} onOpen={open} onClear={() => { onQueryChange(""); search.current?.focus(); }} />
        ) : (
        <>
        {data.note && (
          <div role="status" className="v2-banner v2-banner-warn">
            <IconTile tone="yellow" icon="warning" size={32} />
            <div className="v2-banner-text">
              <strong>Some readings aren’t current</strong>
              <span>{data.note}</span>
            </div>
          </div>
        )}
        <section aria-label="System readings" className="dash-readings">
          {data.metrics.map((metric) => <MetricCard key={metric.label} metric={metric} />)}
        </section>
        {alerts}

        <div className="dash-pair">
          <section aria-labelledby="dash-storage" className="v2-card dash-storage">
            <div className="dash-card-head">
              <h2 id="dash-storage" className="v2-card-title">Storage overview</h2>
              {storage.categories.length > 0 && <LinkButton href={hrefFor("storage")} size="sm">Open Storage</LinkButton>}
            </div>
            <span className="v2-card-note dash-storage-note">
              {storage.volume}
              {storage.lastScan ? ` · analyzed ${storage.lastScan}, read-only` : storage.totalGb > 0 ? ` · ${Math.round(storage.usedGb)} GB used of ${Math.round(storage.totalGb)} GB` : ""}
            </span>
            {storage.categories.length > 0 ? (
              <div className="dash-storage-body">
                <StorageDonut {...storage} />
                <ul className="dash-legend">
                  {storage.categories.map((category) => (
                    <li key={category.label}>
                      <Dot tone={category.tone} />
                      <span>{category.label}</span>
                      <span className="dash-legend-size">{formatGb(category.gb)}</span>
                      <span className="v2-muted">{storage.usedGb > 0 ? `${Math.round((category.gb / storage.usedGb) * 100)}%` : ""}</span>
                    </li>
                  ))}
                </ul>
              </div>
            ) : (
              <div className="dash-empty">
                <p>{storage.analyzing ? "Analyzing what’s taking space… This only reads; nothing changes." : "See what’s taking space by category. Analyzing only reads; nothing changes."}</p>
                {!storage.analyzing && onAnalyze && <Button variant="soft" size="sm" onClick={onAnalyze}>Analyze</Button>}
              </div>
            )}
          </section>

          <section aria-labelledby="dash-actions" className="v2-card dash-list">
            <h2 id="dash-actions" className="v2-card-title dash-list-title">Quick actions</h2>
            {actions.map((action) => (
              <a key={action.title} className="v2-row-action" href={hrefFor(action.route)}>
                <IconTile tone={action.tone} icon={action.icon} size={34} />
                <span className="v2-row-text">
                  <span className="v2-row-title">{action.title}</span>
                  <span className="dash-row-sub">{action.sub}</span>
                </span>
                <span className="v2-chevron"><Glyph name="chevronRight" size={14} /></span>
              </a>
            ))}
          </section>
        </div>

        <div className="dash-pair">
          <section aria-labelledby="dash-findings" className="v2-card dash-list">
            <div className="dash-card-head dash-list-title">
              <h2 id="dash-findings" className="v2-card-title">Recent findings</h2>
              <a className="v2-link" href={hrefFor("review")}>See all</a>
            </div>
            {data.findings.length === 0 && (
              <div className="dash-empty">
                <p>{data.findingsEmpty ?? "No findings yet."}</p>
                <LinkButton href={hrefFor("cleanup")} size="sm">Scan</LinkButton>
              </div>
            )}
            {data.findings.map((finding) => (
              <a key={finding.title} className="v2-row-action" href={hrefFor("review")}>
                <IconTile tone={finding.tone} icon={finding.icon} size={32} />
                <span className="v2-row-text">
                  <span className="v2-row-title">{finding.title}</span>
                  <span className="dash-row-sub">{finding.sub}</span>
                </span>
                <span className="v2-row-size">{formatGb(finding.gb)}</span>
                <span className="v2-chevron"><Glyph name="chevronRight" size={14} /></span>
              </a>
            ))}
          </section>

          <section aria-labelledby="dash-hero" className="dash-hero">
            <svg aria-hidden="true" className="dash-hero-ridges" width="100%" height="120" viewBox="0 0 400 120" preserveAspectRatio="none">
              <path d="M0 90 L60 50 L110 75 L170 25 L230 70 L290 40 L340 65 L400 35 L400 120 L0 120 Z" />
              <path d="M0 105 L80 75 L150 98 L220 70 L300 100 L360 82 L400 95 L400 120 L0 120 Z" />
            </svg>
            <div className="dash-hero-shade" aria-hidden="true" />
            <h2 id="dash-hero">Keep your Mac tidy, on your terms</h2>
            <p>Scans only read. You see exactly what would change and approve every step.</p>
            <LinkButton href={hrefFor("cleanup")} variant="primary" className="dash-hero-cta">Start scan →</LinkButton>
          </section>
        </div>
        </>
        )}
      </ScrollBody>
    </>
  );
}
