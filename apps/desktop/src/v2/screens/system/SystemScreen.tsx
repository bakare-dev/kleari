import { useState } from "react";
import type { IconName } from "../../ui/icons";
import {
  Button,
  chartSegments,
  GaugeRing,
  IconTile,
  LiveBadge,
  Modal,
  PageHeader,
  Pill,
  ScrollBody,
  Segmented,
  Toast,
  useToast,
  type LiveState,
  type Status,
  type Tone,
} from "../../ui/components";
import { headingIdFor } from "../../shell/routes";
import { Glyph, StatusMark } from "../../ui/icons";
import "./system.css";

export type { LiveState };

export type ChartTab = "cpu" | "memory" | "network" | "disk";
export type SettingsPane = "softwareUpdate" | "loginItems";

export interface GaugeView {
  label: string;
  tone: Tone;
  /** `null` when there is no reading: the ring shows its track only. */
  fraction: number | null;
  center: string;
  value: string;
  sub: string;
  pill: string;
  status: Status;
  aria: string;
}

export interface ChartSeriesView {
  name: string;
  /**
   * One entry per sample, oldest first, in the chart's unit; `null` where that sample had no
   * reading. Only samples taken while the page was open.
   */
  values: readonly (number | null)[];
  now: string;
  tone: Tone;
}

export interface ChartView {
  max: number;
  scale: string;
  note: string;
  series: readonly ChartSeriesView[];
}

export interface HealthCheck {
  label: string;
  sub: string;
  status?: string;
  tone: Status;
  tile: Tone;
  icon: IconName;
  /** Opens the matching pane in System Settings. */
  action?: { label: string; pane: SettingsPane };
}

export interface StartupRow {
  name: string;
  kind: string;
  status: string;
  tone: Status;
  tile: Tone;
}

export interface StartupView {
  state: "loading" | "ready" | "unavailable";
  message?: string;
  rows: readonly StartupRow[];
  summary: string;
  counts: string;
}

export interface SystemView {
  live: LiveState;
  /** Shown above the gauges when readings fail or need the desktop app. */
  note: string | null;
  gauges: readonly GaugeView[];
  charts: Record<ChartTab, ChartView>;
  /** Positions on the chart's time axis, one per sample. */
  slots: number;
  /** How long ago the oldest charted reading was taken ("45 s", "2 min"); null with fewer than two. */
  span: string | null;
  mac: { title: string; detail: string };
  specs: readonly { label: string; value: string }[];
  health: readonly HealthCheck[];
  healthSummary: string;
  startup: StartupView;
}

interface SystemScreenProps {
  view: SystemView;
  paused: boolean;
  onTogglePause: () => void;
  /** Opens a System Settings pane; resolves to whether macOS opened it. */
  onOpenSettings: (pane: SettingsPane) => Promise<boolean>;
}

const tabLabels: Record<ChartTab, string> = { cpu: "CPU", memory: "Memory", network: "Network", disk: "Disk" };

function BackgroundRow({ item, wide = false }: { item: StartupRow; wide?: boolean }) {
  return (
    <div className={`sys-bg-row${wide ? " wide" : ""}`}>
      <IconTile tone={item.tile} initial={item.name.charAt(0).toUpperCase()} size={wide ? 28 : 26} />
      <span className="v2-row-text">
        <span className="v2-row-title">{item.name}</span>
        <span className="v2-row-sub">{item.kind}</span>
      </span>
      <span className="sys-bg-status"><Pill status={item.tone}>{item.status}</Pill></span>
    </div>
  );
}

export function SystemScreen({ view, paused, onTogglePause, onOpenSettings }: SystemScreenProps) {
  const [tab, setTab] = useState<ChartTab>("cpu");
  const [allItemsOpen, setAllItemsOpen] = useState(false);
  const toast = useToast();
  const chart = view.charts[tab];
  const sampleCount = Math.max(0, ...chart.series.map((entry) => entry.values.filter((value) => value !== null).length));
  const shown = view.startup.rows.slice(0, 5);

  const open = (pane: SettingsPane) => {
    void onOpenSettings(pane).then((opened) => {
      if (!opened) {
        toast.show("System Settings opens from the Kleari desktop app.", "info");
      }
    });
  };

  return (
    <>
      <PageHeader
        title="System"
        headingId={headingIdFor("system")}
        subtitle="Live readings from this Mac. Kleari only reads these; it doesn’t change system settings."
        actions={
          <>
            <LiveBadge state={view.live} />
            <Button variant="soft" size="sm" onClick={onTogglePause} disabled={view.live === "unavailable"}>{paused ? "Resume" : "Pause"}</Button>
          </>
        }
      />
      <ScrollBody label="System">
        {/* Always mounted, so a change to stale or unavailable is announced. */}
        <div role="status" className="sys-note-region">
          {view.note && (
            <div className="v2-info-strip sys-note">
              <StatusMark tone="warn" />
              <span>{view.note}</span>
            </div>
          )}
        </div>

        <section aria-label="Live readings" className="v2-grid-4">
          {view.gauges.map((gauge) => (
            <div key={gauge.label} className="v2-card sys-gauge">
              <div className="sys-gauge-head">
                <span className="sys-gauge-label">{gauge.label}</span>
                <Pill status={gauge.status} className="sys-gauge-pill">{gauge.pill}</Pill>
              </div>
              <div className="sys-gauge-body">
                <GaugeRing fraction={gauge.fraction} tone={gauge.tone} label={gauge.aria}>{gauge.center}</GaugeRing>
                <span className="v2-row-text sys-gauge-text">
                  <span className="sys-gauge-value">{gauge.value}</span>
                  <span className="sys-gauge-sub">{gauge.sub}</span>
                </span>
              </div>
            </div>
          ))}
        </section>

        <div className="sys-pair">
          <section aria-labelledby="sys-chart" className="v2-card sys-chart">
            <div className="sys-chart-head">
              <h2 id="sys-chart" className="v2-card-title-sm">{view.span ? `Activity · last ${view.span}` : "Activity"}</h2>
              <Segmented
                label="Chart"
                value={tab}
                onChange={setTab}
                options={(Object.keys(tabLabels) as ChartTab[]).map((key) => ({ value: key, label: tabLabels[key] }))}
              />
            </div>
            <div className="sys-chart-legend">
              {chart.series.map((entry) => (
                <span key={entry.name} className={`tone-${entry.tone}`}>
                  <span className="sys-legend-dot" aria-hidden="true" />
                  <span className="v2-muted">{entry.name}</span>
                  <strong className="v2-num">{entry.now}</strong>
                </span>
              ))}
              <span className="sys-chart-note">{chart.note}</span>
            </div>
            <div className="sys-chart-frame">
              <svg
                role="img"
                aria-label={`${tabLabels[tab]}${view.span ? ` over the last ${view.span}` : ""}, ${sampleCount} ${sampleCount === 1 ? "reading" : "readings"}${chart.series.map((entry) => `, ${entry.name} now ${entry.now}`).join("")}`}
                width="100%"
                height="150"
                viewBox="0 0 480 150"
                preserveAspectRatio="none"
                className="sys-chart-svg"
              >
                <defs>
                  {chart.series.map((entry, index) => (
                    <g key={entry.name} className={`tone-${entry.tone}`}>
                      <linearGradient id={`sys-area-${tab}-${index}`} x1="0" y1="0" x2="0" y2="1">
                        <stop offset="0" className="v2-stop-2" stopOpacity="0.32" />
                        <stop offset="1" className="v2-stop-2" stopOpacity="0" />
                      </linearGradient>
                      <linearGradient id={`sys-line-${tab}-${index}`} x1="0" y1="0" x2="1" y2="0">
                        <stop offset="0" className="v2-stop-1" />
                        <stop offset="1" className="v2-stop-2" />
                      </linearGradient>
                    </g>
                  ))}
                </defs>
                {[14, 82, 149].map((y) => <line key={y} x1="0" x2="480" y1={y} y2={y} className="sys-grid" strokeWidth="1" vectorEffect="non-scaling-stroke" />)}
                {chart.series.map((entry, index) => (
                  <g key={entry.name} className={`tone-${entry.tone}`}>
                    {chartSegments(entry.values, view.slots, chart.max).map((segment) => (
                      <g key={segment.firstX}>
                        <polygon points={`${segment.firstX},150 ${segment.points} ${segment.lastX},150`} fill={`url(#sys-area-${tab}-${index})`} />
                        <polyline points={segment.points} fill="none" stroke={`url(#sys-line-${tab}-${index})`} strokeWidth="2.4" strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
                      </g>
                    ))}
                  </g>
                ))}
              </svg>
              {sampleCount < 2 && (
                <span className="sys-chart-empty">
                  {view.live === "unavailable" ? "No readings" : view.live === "paused" ? "Paused. Resume to take readings." : "Readings appear here in a few seconds."}
                </span>
              )}
            </div>
            <div className="sys-chart-axis"><span>{view.span ? `${view.span} ago` : ""}</span><span>{chart.scale}</span><span>Now</span></div>
          </section>

          <section aria-labelledby="sys-mac" className="v2-card sys-mac">
            <div className="sys-mac-head">
              <IconTile tone="gray" icon="laptop" size={46} glow />
              <span className="v2-row-text">
                <h2 id="sys-mac" className="v2-card-title-sm">{view.mac.title}</h2>
                <span className="v2-muted sys-mac-sub">{view.mac.detail}</span>
              </span>
            </div>
            <dl className="sys-specs">
              {view.specs.map((spec) => (
                <div key={spec.label}>
                  <dt>{spec.label}</dt>
                  <dd>{spec.value}</dd>
                </div>
              ))}
            </dl>
          </section>
        </div>

        <div className="sys-pair sys-pair-even">
          <section aria-labelledby="sys-health" className="v2-card sys-list">
            <div className="sys-list-head">
              <h2 id="sys-health" className="v2-card-title-sm">Health checks</h2>
              <span className="v2-card-note">{view.healthSummary}</span>
            </div>
            {view.health.map((check) => (
              <div key={check.label} className="v2-row">
                <IconTile tone={check.tile} icon={check.icon} size={30} />
                <span className="v2-row-text">
                  <span className="v2-row-title">{check.label}</span>
                  <span className="v2-row-sub v2-wrap">{check.sub}</span>
                </span>
                {check.action && (
                  <Button variant="soft" size="sm" onClick={() => check.action && open(check.action.pane)}>{check.action.label}</Button>
                )}
                {check.status && <Pill status={check.tone}>{check.status}</Pill>}
              </div>
            ))}
          </section>

          <section aria-labelledby="sys-bg" className="v2-card sys-list">
            <div className="sys-list-head">
              <h2 id="sys-bg" className="v2-card-title-sm">Startup and background items</h2>
              <Button variant="soft" size="sm" onClick={() => open("loginItems")}>Manage in System Settings</Button>
            </div>
            {view.startup.state === "ready" ? (
              <>
                <span className="v2-card-note sys-bg-summary">{view.startup.summary}</span>
                {shown.map((item, index) => <BackgroundRow key={`${item.name}-${index}`} item={item} />)}
                <span className="v2-card-note sys-bg-note">Login items you added yourself are listed in System Settings.</span>
                {view.startup.rows.length > shown.length && (
                  <button type="button" className="sys-more" onClick={() => setAllItemsOpen(true)}>Show all {view.startup.rows.length} items</button>
                )}
              </>
            ) : (
              <span className="v2-card-note sys-bg-summary">{view.startup.message}</span>
            )}
          </section>
        </div>
      </ScrollBody>

      <Toast message={toast.message} />

      <Modal open={allItemsOpen} onClose={() => setAllItemsOpen(false)} labelledBy="sys-all-title" className="sys-all">
        <div className="v2-dialog-body sys-all-body">
          <div className="v2-dialog-head">
            <IconTile tone="indigo" icon="power" size={46} glow />
            <div className="v2-row-text">
              <h2 id="sys-all-title" className="v2-dialog-title">Startup and background items</h2>
              <span className="v2-dialog-text">{view.startup.counts}</span>
            </div>
            <button type="button" className="v2-close" aria-label="Close" onClick={() => setAllItemsOpen(false)}><Glyph name="close" size={12} strokeWidth={2.6} /></button>
          </div>
          <div className="v2-info-strip sys-all-info">
            <StatusMark tone="info" size={15} />
            <span>Kleari lists launch agents and daemons so you can see what runs in the background. Turning one off is done in System Settings.</span>
          </div>
          <div className="sys-all-list">
            {view.startup.rows.map((item, index) => <BackgroundRow key={`${item.name}-${index}`} item={item} wide />)}
          </div>
          <div className="v2-dialog-actions-end">
            <Button variant="secondary" onClick={() => open("loginItems")}>Manage in System Settings</Button>
            <Button variant="primary" onClick={() => setAllItemsOpen(false)}>Done</Button>
          </div>
        </div>
      </Modal>
    </>
  );
}
