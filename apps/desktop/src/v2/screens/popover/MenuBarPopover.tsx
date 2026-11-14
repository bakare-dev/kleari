import { Glyph, type IconName } from "../../ui/icons";
import { Button, IconTile, type Tone } from "../../ui/components";
import { NavigatorIcon, Wordmark } from "../../shell/brand";
import { hrefFor } from "../../shell/routes";
import type { MenuBarDestination } from "../../../lib/menubarIpc";
import "./popover.css";

export interface PopoverGauge {
  label: string;
  value: string;
  sub: string;
  /** 0–100, or `null` before a reading. */
  percent: number | null;
  route: MenuBarDestination;
  tone: Tone;
  icon: IconName;
}

export interface PopoverLink {
  label: string;
  sub: string;
  value: string;
  route: MenuBarDestination;
  tone: Tone;
  icon: IconName;
}

export interface PopoverSummary {
  title: string;
  sub: string;
  tone: Tone;
  icon: IconName;
  /** The review button's label, or `null` when there is nothing to review. */
  review: string | null;
}

/** Download and upload now; `note` says why there are no figures yet. */
export interface PopoverNetwork {
  down: string;
  up: string;
  note: string | null;
}

/** One issue alert, as the Dashboard shows it. */
export interface PopoverAlert {
  key: string;
  title: string;
  sub: string;
  warning: boolean;
  route: MenuBarDestination;
  tone: Tone;
  icon: IconName;
}

/** Alerts shown at most; the rest are on the Dashboard. */
const MAX_ALERTS = 2;

interface MenuBarPopoverProps {
  gauges: readonly PopoverGauge[];
  network: PopoverNetwork;
  alerts: readonly PopoverAlert[];
  summary: PopoverSummary;
  links: readonly PopoverLink[];
  /** The last action couldn't reach Kleari. */
  failed?: boolean;
  /** Opens the main window at `route` (and closes the popover). */
  onGo: (route: MenuBarDestination) => void;
  onQuit: () => void;
}

/**
 * The menu-bar popover from the v2 design, shown below Kleari's menu-bar icon. It only
 * reads: links open the matching screen in the main window, and nothing here starts a scan.
 */
export function MenuBarPopover({ gauges, network, alerts, summary, links, failed = false, onGo, onQuit }: MenuBarPopoverProps) {
  const shownAlerts = alerts.slice(0, MAX_ALERTS);
  const moreAlerts = alerts.length - shownAlerts.length;
  const link = (route: MenuBarDestination) => ({
    href: hrefFor(route),
    onClick: (event: { preventDefault: () => void }) => {
      event.preventDefault();
      onGo(route);
    },
  });
  return (
    <section className="pop pop-window" aria-labelledby="pop-title">
      <div className="pop-head">
        <NavigatorIcon size={34} />
        <span className="v2-row-text">
          <h2 id="pop-title"><Wordmark className="pop-wordmark" /></h2>
          <span className="pop-offline"><span className="v2-online-dot" aria-hidden="true" />Your files never leave this Mac</span>
        </span>
        <button type="button" className="pop-icon-btn" aria-label="Settings" onClick={() => onGo("settings")}><Glyph name="sliders" size={15} /></button>
      </div>
      <div className="pop-body">
        <div className="pop-gauges">
          {gauges.map((gauge) => (
            <a key={gauge.label} className={`pop-tile pop-gauge tone-${gauge.tone}`} {...link(gauge.route)}>
              <span className="pop-gauge-head">
                <IconTile tone={gauge.tone} icon={gauge.icon} size={22} glow />
                <span className="v2-muted pop-gauge-label">{gauge.label}</span>
              </span>
              <span className="pop-gauge-value">{gauge.value}</span>
              <span className="pop-gauge-track" aria-hidden="true"><span style={{ width: `${gauge.percent ?? 0}%` }} /></span>
              <span className="v2-row-sub v2-wrap">{gauge.sub}</span>
            </a>
          ))}
        </div>
        <a className="pop-tile pop-network" aria-label={`Network: download ${network.down}, upload ${network.up}`} {...link("system")}>
          <IconTile tone="teal" icon="network" size={22} glow />
          <span className="v2-muted pop-gauge-label">Network</span>
          {network.note ? (
            <span className="v2-row-sub pop-network-note">{network.note}</span>
          ) : (
            <span className="pop-network-rates">
              <span className="pop-rate"><span aria-hidden="true" className="pop-rate-arrow">↓</span><span className="v2-num">{network.down}</span></span>
              <span className="pop-rate"><span aria-hidden="true" className="pop-rate-arrow">↑</span><span className="v2-num">{network.up}</span></span>
            </span>
          )}
        </a>
        {shownAlerts.length > 0 && (
          <div className="pop-tile pop-links pop-alerts" role="list" aria-label="Worth your attention">
            {shownAlerts.map((alert) => (
              <a key={alert.key} role="listitem" className="pop-link" {...link(alert.route)}>
                <IconTile tone={alert.tone} icon={alert.icon} size={26} />
                <span className="v2-row-text">
                  <span className="v2-row-title">{alert.title}</span>
                  <span className="v2-row-sub">{alert.sub}</span>
                </span>
                {alert.warning && <span className="pop-alert-dot" aria-label="Warning" />}
                <span className="v2-chevron"><Glyph name="chevronRight" size={12} strokeWidth={2.2} /></span>
              </a>
            ))}
            {moreAlerts > 0 && (
              <a className="pop-more v2-link" {...link("dashboard")}>
                {moreAlerts} more on the Dashboard
              </a>
            )}
          </div>
        )}
        <div className="pop-tile pop-scan">
          <div className="pop-scan-head">
            <IconTile tone={summary.tone} icon={summary.icon} size={34} glow />
            <span className="v2-row-text">
              <strong>{summary.title}</strong>
              <span className="v2-row-sub v2-wrap pop-scan-sub">{summary.sub}</span>
            </span>
          </div>
          <div className="pop-scan-actions">
            <Button variant="primary" onClick={() => onGo("cleanup")}>Open Cleanup</Button>
            {summary.review && <Button variant="soft" onClick={() => onGo("review")}>{summary.review}</Button>}
          </div>
        </div>
        <div className="pop-tile pop-links">
          {links.map((entry) => (
            <a key={entry.label} className="pop-link" {...link(entry.route)}>
              <IconTile tone={entry.tone} icon={entry.icon} size={26} />
              <span className="v2-row-text">
                <span className="v2-row-title">{entry.label}</span>
                <span className="v2-row-sub">{entry.sub}</span>
              </span>
              <span className="pop-link-value v2-num">{entry.value}</span>
              <span className="v2-chevron"><Glyph name="chevronRight" size={12} strokeWidth={2.2} /></span>
            </a>
          ))}
        </div>
      </div>
      {failed && (
        <p role="alert" className="v2-row-sub pop-failed">
          Kleari didn’t respond. Try again, or open it from Applications.
        </p>
      )}
      <div className="pop-foot">
        <Button variant="secondary" className="pop-open" onClick={() => onGo("dashboard")}>Open Kleari</Button>
        <Button variant="plain" onClick={onQuit}>Quit</Button>
      </div>
    </section>
  );
}
