import { chartSegments, type LiveState } from "../../ui/components";
import type { ChartView } from "./SystemScreen";
import "./system.css";

interface LiveChartProps {
  chart: ChartView;
  /** What it shows, for its label ("CPU", "Network"). */
  name: string;
  /** Unique on the page: names its gradients. */
  id: string;
  slots: number;
  /** How long ago the oldest charted reading was taken; null with fewer than two. */
  span: string | null;
  live: LiveState;
}

/** A live readings chart: legend with the latest figures, the lines, and the time axis.
 * Shared by System and Activity's Network tab. */
export function LiveChart({ chart, name, id, slots, span, live }: LiveChartProps) {
  const sampleCount = Math.max(0, ...chart.series.map((entry) => entry.values.filter((value) => value !== null).length));
  return (
    <>
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
          aria-label={`${name}${span ? ` over the last ${span}` : ""}, ${sampleCount} ${sampleCount === 1 ? "reading" : "readings"}${chart.series.map((entry) => `, ${entry.name} now ${entry.now}`).join("")}`}
          width="100%"
          height="150"
          viewBox="0 0 480 150"
          preserveAspectRatio="none"
          className="sys-chart-svg"
        >
          <defs>
            {chart.series.map((entry, index) => (
              <g key={entry.name} className={`tone-${entry.tone}`}>
                <linearGradient id={`${id}-area-${index}`} x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0" className="v2-stop-2" stopOpacity="0.32" />
                  <stop offset="1" className="v2-stop-2" stopOpacity="0" />
                </linearGradient>
                <linearGradient id={`${id}-line-${index}`} x1="0" y1="0" x2="1" y2="0">
                  <stop offset="0" className="v2-stop-1" />
                  <stop offset="1" className="v2-stop-2" />
                </linearGradient>
              </g>
            ))}
          </defs>
          {[14, 82, 149].map((y) => <line key={y} x1="0" x2="480" y1={y} y2={y} className="sys-grid" strokeWidth="1" vectorEffect="non-scaling-stroke" />)}
          {chart.series.map((entry, index) => (
            <g key={entry.name} className={`tone-${entry.tone}`}>
              {chartSegments(entry.values, slots, chart.max).map((segment) => (
                <g key={segment.firstX}>
                  <polygon points={`${segment.firstX},150 ${segment.points} ${segment.lastX},150`} fill={`url(#${id}-area-${index})`} />
                  <polyline points={segment.points} fill="none" stroke={`url(#${id}-line-${index})`} strokeWidth="2.4" strokeLinejoin="round" strokeLinecap="round" vectorEffect="non-scaling-stroke" />
                </g>
              ))}
            </g>
          ))}
        </svg>
        {sampleCount < 2 && (
          <span className="sys-chart-empty">
            {live === "unavailable" ? "No readings" : live === "paused" ? "Paused. Resume to take readings." : "Readings appear here in a few seconds."}
          </span>
        )}
      </div>
      <div className="sys-chart-axis"><span>{span ? `${span} ago` : ""}</span><span>{chart.scale}</span><span>Now</span></div>
    </>
  );
}
