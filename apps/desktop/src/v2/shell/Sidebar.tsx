import { Glyph } from "../ui/icons";
import { NavigatorIcon, Wordmark } from "./brand";
import { hrefFor, mainNav, navRouteFor, settingsNav, type NavDestination, type V2Route } from "./routes";
import type { ReactNode } from "react";

function NavItem({ destination, current }: { destination: NavDestination; current: boolean }) {
  return (
    <a className="v2-nav-item" href={hrefFor(destination.route)} aria-current={current ? "page" : undefined}>
      <Glyph name={destination.icon} size={17} strokeWidth={1.9} />
      <span>{destination.label}</span>
    </a>
  );
}

interface SidebarProps {
  route: V2Route;
  /** The main destinations shown; release builds list only the screens they ship. */
  destinations?: readonly NavDestination[];
  /** Development-only controls (the sample-data badge), rendered under the offline chip. */
  footer?: ReactNode;
}

export function Sidebar({ route, destinations = mainNav, footer }: SidebarProps) {
  const selected = navRouteFor(route);
  return (
    <nav className="v2-sidebar" aria-label="Main">
      {/* Space for the window controls; part of the window's drag area in the desktop app. */}
      <div className="v2-sidebar-controls" data-tauri-drag-region aria-hidden="true" />
      <div className="v2-brand">
        <NavigatorIcon size={36} />
        <span className="v2-brand-text">
          <Wordmark />
          <span className="v2-tagline">Scan · Explain · Approve</span>
        </span>
      </div>
      <div className="v2-nav">
        {destinations.map((destination) => (
          <NavItem key={destination.route} destination={destination} current={selected === destination.route} />
        ))}
      </div>
      <div className="v2-sidebar-spacer" />
      <NavItem destination={settingsNav} current={selected === "settings"} />
      <div className="v2-offline-chip">
        <span className="v2-online-dot" aria-hidden="true" />
        <span>Local. Your files never leave this Mac.</span>
      </div>
      {footer}
    </nav>
  );
}
