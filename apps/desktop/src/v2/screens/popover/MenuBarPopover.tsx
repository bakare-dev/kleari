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

interface MenuBarPopoverProps {
  gauges: readonly PopoverGauge[];
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
export function MenuBarPopover({ gauges, summary, links, failed = false, onGo, onQuit }: MenuBarPopoverProps) {
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
