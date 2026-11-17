import type { ReactNode } from "react";
import { Button, LiveBadge, PageHeader, StatCard, type LiveState } from "../../ui/components";
import { StatusMark } from "../../ui/icons";
import { headingIdFor } from "../../shell/routes";
import { LiveChart } from "../system/LiveChart";
import type { ChartView } from "../system/SystemScreen";
import "./activity.css";

export interface NetworkView {
  live: LiveState;
  /** Why readings are missing or old, if they are. */
  note: string | null;
  /** Download and upload now. */
  down: string;
  up: string;
  /** The highest of each in the chart. */
  peakDown: string;
  peakUp: string;
  chart: ChartView;
  slots: number;
  span: string | null;
}

interface NetworkScreenProps {
  view: NetworkView;
  paused: boolean;
  onTogglePause: () => void;
  tabs: ReactNode;
}

/** Activity's Network tab: this Mac's download and upload rates, read only while it is shown. */
export function NetworkScreen({ view, paused, onTogglePause, tabs }: NetworkScreenProps) {
  const span = view.span ? `in the last ${view.span}` : "since this tab opened";
  return (
    <>
      <PageHeader
        title="Activity"
        headingId={headingIdFor("activity")}
        subtitle="This Mac’s network traffic, all apps together, read every second while this tab is open. Nothing is stored."
        actions={
          <>
            {tabs}
            <LiveBadge state={view.live} />
            <Button variant="soft" size="sm" onClick={onTogglePause} disabled={view.live === "unavailable"}>{paused ? "Resume" : "Pause"}</Button>
          </>
        }
      />
      <div className="act-body act-network">
        {/* Always mounted, so a change to stale or unavailable is announced. */}
        <div role="status">
          {view.note && (
            <div className="v2-info-strip">
              <StatusMark tone="warn" />
              <span>{view.note}</span>
            </div>
          )}
        </div>
        <section aria-label="Network now" className="v2-grid-4">
          <StatCard tone="blue" icon="download" label="Downloading" value={view.down} sub="now" />
          <StatCard tone="teal" icon="network" label="Uploading" value={view.up} sub="now" />
          <StatCard tone="indigo" icon="download" label="Fastest download" value={view.peakDown} sub={span} />
          <StatCard tone="green" icon="network" label="Fastest upload" value={view.peakUp} sub={span} />
        </section>
        <section aria-labelledby="act-net-chart" className="v2-card sys-chart">
          <div className="sys-chart-head">
            <h2 id="act-net-chart" className="v2-card-title-sm">{view.span ? `Network · last ${view.span}` : "Network"}</h2>
          </div>
          <LiveChart chart={view.chart} name="Network" id="act-net" slots={view.slots} span={view.span} live={view.live} />
        </section>
      </div>
    </>
  );
}
