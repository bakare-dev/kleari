import type { Issue, IssueKind } from "../../../contracts/issues";
import type { IconName } from "../../ui/icons";
import { IconTile, Pill, type Tone } from "../../ui/components";
import { formatGb } from "../../ui/format";
import { hrefFor, type V2Route } from "../../shell/routes";

const look: Record<IssueKind, { icon: IconName; tone: Tone; route: V2Route; action: string }> = {
  lowDiskSpace: { icon: "disk", tone: "red", route: "storage", action: "See what takes space" },
  lowDriveSpace: { icon: "disk", tone: "orange", route: "storage", action: "Open Storage" },
  heavyMemory: { icon: "chip", tone: "orange", route: "processes", action: "Open Processes" },
  batteryDrain: { icon: "battery", tone: "orange", route: "processes", action: "See what is busy" },
  lowDeviceBattery: { icon: "battery", tone: "yellow", route: "system", action: "Open System" },
  fullTrash: { icon: "trashLines", tone: "blue", route: "maintenance", action: "Empty in Maintenance" },
  unusedApps: { icon: "apps", tone: "violet", route: "apps", action: "Review in Apps" },
};

const gb = (bytes: number | null) => formatGb((bytes ?? 0) / 1e9);

function text(issue: Issue): { title: string; sub: string } {
  switch (issue.kind) {
    case "lowDiskSpace":
      return { title: "Your startup disk is almost full", sub: `${gb(issue.value)} free of ${gb(issue.total)}` };
    case "lowDriveSpace":
      return { title: `${issue.subject ?? "A drive"} is almost full`, sub: `${gb(issue.value)} free of ${gb(issue.total)}` };
    case "heavyMemory":
      return { title: "Memory is under pressure", sub: issue.value === 4 ? "Critical: apps may stall" : "Apps may slow down" };
    case "batteryDrain":
      return { title: "Your battery is draining fast", sub: `About ${issue.total ?? 0} W on battery` };
    case "lowDeviceBattery":
      return { title: `${issue.subject ?? "A Bluetooth device"} battery is low`, sub: `${issue.value ?? 0}% left` };
    case "fullTrash":
      return { title: "The Trash is getting large", sub: `${gb(issue.value)} in the Trash` };
    case "unusedApps":
      return { title: `${issue.value ?? 0} apps not opened in 6 months`, sub: issue.subject ?? "" };
  }
}

/** Issue alerts on the Dashboard: what is worth attention now, each with where to deal with it. */
export function IssuesCard({ issues, failed }: { issues: Issue[] | null; failed: boolean }) {
  if (failed || !issues || issues.length === 0) {
    return null;
  }
  return (
    <section aria-labelledby="dash-issues" className="v2-card dash-issues">
      <h2 id="dash-issues" className="v2-card-title-sm">Worth your attention</h2>
      <ul className="dash-issue-rows">
        {issues.map((issue, index) => {
          const meta = look[issue.kind];
          const { title, sub } = text(issue);
          return (
            <li key={`${issue.kind}-${issue.subject ?? index}`}>
              <IconTile tone={meta.tone} icon={meta.icon} size={30} />
              <span className="v2-row-text">
                <span className="v2-row-title">{title}</span>
                <span className="v2-row-sub v2-ellipsis">{sub}</span>
              </span>
              {issue.severity === "warning" && <Pill status="warn">Warning</Pill>}
              <a className="v2-link" href={hrefFor(meta.route)}>{meta.action}</a>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
