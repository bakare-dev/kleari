import { useState } from "react";
import { Glyph, type IconName } from "../../ui/icons";
import { Button, IconTile, PageHeader, Pill, SearchField, Segmented, StatCard, type Status, type Tone } from "../../ui/components";
import { hrefFor, headingIdFor, type V2Route } from "../../shell/routes";
import "./activity.css";

export type ActivityKind = "scan" | "quarantine" | "restore" | "blocked" | "delete" | "review" | "approve" | "system";
export type ActivityGroup = "scan" | "quarantine" | "restore" | "delete" | "system";

export interface ActivityLine {
  name: string;
  path?: string;
  note?: string;
  tone?: "warn" | "danger" | "ok";
}

export interface ActivityEvent {
  id: string;
  time: string;
  kind: ActivityKind;
  icon: IconName;
  title: string;
  sub: string;
  size: string;
  lines: readonly ActivityLine[];
  link?: { route: V2Route; label: string };
}

export interface ActivityDay {
  label: string;
  sub: string;
  events: readonly ActivityEvent[];
}

export interface ActivityStat {
  label: string;
  value: string;
  sub: string;
  tone: Tone;
  icon: IconName;
}

interface ActivityScreenProps {
  days: readonly ActivityDay[];
  stats: readonly ActivityStat[];
  retentionDays: number;
  /** Shown only when given (export needs its own privacy review before it ships). */
  onExport?: () => void;
  /** Why the log can't be read now; nothing is shown as having happened. */
  unavailable?: string | null;
  /** More happened in the kept window than is listed. */
  truncated?: boolean;
  /** The log hasn't been read yet: show nothing rather than an empty log. */
  loading?: boolean;
}

const kindMeta: Record<ActivityKind, { tag: string; tone: Tone; status: Status; group: ActivityGroup }> = {
  scan: { tag: "Scan", tone: "blue", status: "link", group: "scan" },
  review: { tag: "Sent to review", tone: "indigo", status: "link", group: "scan" },
  approve: { tag: "Approved", tone: "indigo", status: "ok", group: "scan" },
  quarantine: { tag: "Quarantined", tone: "orange", status: "warn", group: "quarantine" },
  restore: { tag: "Restored", tone: "green", status: "ok", group: "restore" },
  blocked: { tag: "Restore blocked", tone: "yellow", status: "warn", group: "restore" },
  delete: { tag: "Deleted for good", tone: "red", status: "danger", group: "delete" },
  system: { tag: "System", tone: "gray", status: "muted", group: "system" },
};

const filters: readonly { value: ActivityGroup | "all"; label: string }[] = [
  { value: "all", label: "All" },
  { value: "scan", label: "Scans" },
  { value: "quarantine", label: "Quarantine" },
  { value: "restore", label: "Restores" },
  { value: "delete", label: "Deletes" },
  { value: "system", label: "System" },
];

export function ActivityScreen({ days, stats, retentionDays, onExport, unavailable, truncated, loading }: ActivityScreenProps) {
  const [filter, setFilter] = useState<ActivityGroup | "all">("all");
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());

  const needle = query.trim().toLowerCase();
  const matches = (event: ActivityEvent) =>
    (filter === "all" || kindMeta[event.kind].group === filter) &&
    (needle === "" ||
      `${event.title} ${event.sub} ${event.lines.map((line) => `${line.name} ${line.path ?? ""}`).join(" ")}`.toLowerCase().includes(needle));
  const shown = days.map((day) => ({ ...day, events: day.events.filter(matches) })).filter((day) => day.events.length > 0);
  const count = (group: ActivityGroup | "all") =>
    days.reduce((total, day) => total + day.events.filter((event) => group === "all" || kindMeta[event.kind].group === group).length, 0);

  const toggle = (id: string) => {
    const next = new Set(open);
    if (next.has(id)) {
      next.delete(id);
    } else {
      next.add(id);
    }
    setOpen(next);
  };

  return (
    <>
      <PageHeader
        title="Activity"
        headingId={headingIdFor("activity")}
        subtitle="Everything Kleari has scanned, moved, restored or deleted. This log stays on this Mac."
        actions={
          onExport && (
            <Button variant="soft" size="sm" onClick={onExport}>
              <Glyph name="download" size={13} strokeWidth={2.2} />
              Export log
            </Button>
          )
        }
      />
      <div className="act-body">
        {unavailable && (
          <div role="alert" className="v2-banner v2-banner-danger">
            <IconTile tone="red" icon="warning" size={32} />
            <div className="v2-banner-text">
              <strong>Activity can’t be shown right now</strong>
              <span>{unavailable} Your files and quarantine aren’t affected.</span>
            </div>
          </div>
        )}
        <section aria-label="This week" className="v2-grid-4">
          {stats.map((stat) => <StatCard key={stat.label} tone={stat.tone} icon={stat.icon} label={stat.label} value={stat.value} sub={stat.sub} />)}
        </section>
        <div className="act-toolbar">
          <Segmented
            label="Filter"
            size="md"
            value={filter}
            onChange={setFilter}
            options={filters.map((entry) => ({ value: entry.value, label: `${entry.label} ${count(entry.value)}` }))}
          />
          <SearchField label="Search activity" value={query} onChange={setQuery} width={190} strong />
        </div>
        <div tabIndex={0} role="region" aria-label="Activity log" className="act-log">
          {shown.map((day) => (
            <section key={day.label} aria-label={day.label} className="v2-card act-day">
              <div className="act-day-head">
                <h2>{day.label}</h2>
                <span className="v2-card-note">{day.sub} · {day.events.length} {day.events.length === 1 ? "event" : "events"}</span>
              </div>
              <ol className="act-events">
                {day.events.map((event) => {
                  const meta = kindMeta[event.kind];
                  const isOpen = open.has(event.id) && event.lines.length > 0;
                  return (
                    <li key={event.id}>
                      <div className="act-event">
                        <span className="act-time v2-num">{event.time}</span>
                        <IconTile tone={meta.tone} icon={event.icon} size={30} />
                        <span className="v2-row-text">
                          <span className="v2-row-title">{event.title}</span>
                          <span className="v2-row-sub">{event.sub}</span>
                        </span>
                        <span className="act-size">{event.size}</span>
                        <span className="act-tag"><Pill status={meta.status}>{meta.tag}</Pill></span>
                        <span className="act-more">
                          {event.lines.length > 0 && (
                            <Button variant="soft" size="sm" className="act-toggle" aria-expanded={isOpen} onClick={() => toggle(event.id)}>
                              {isOpen ? "Less" : "Details"}
                              <span style={{ display: "flex", transform: isOpen ? "rotate(180deg)" : "none", transition: "transform 0.2s" }}>
                                <Glyph name="chevronDown" size={11} strokeWidth={2.6} />
                              </span>
                            </Button>
                          )}
                        </span>
                      </div>
                      {isOpen && (
                        <div className="act-lines">
                          {event.lines.map((line) => (
                            <div key={`${line.name}${line.path ?? ""}`} className="act-line">
                              <span className="v2-ellipsis">
                                <strong>{line.name}</strong> {line.path && <span className="v2-mono act-path">{line.path}</span>}
                              </span>
                              <span className={`act-note${line.tone ? ` act-note-${line.tone}` : ""}`}>{line.note}</span>
                            </div>
                          ))}
                          {event.link && <a className="v2-link act-link" href={hrefFor(event.link.route)}>{event.link.label}</a>}
                        </div>
                      )}
                    </li>
                  );
                })}
              </ol>
            </section>
          ))}
          {shown.length === 0 && !unavailable && !loading && (
            <span className="act-empty">
              {days.length === 0 ? "Nothing recorded yet. Scans, moves, restores and deletes appear here as they happen." : "Nothing in the log matches this filter."}
            </span>
          )}
          {truncated && <span className="act-retention">Only the latest 500 events are listed.</span>}
          <span className="act-retention">Kleari keeps {retentionDays} days of activity on this Mac. Older entries are deleted, never sent anywhere.</span>
        </div>
      </div>
    </>
  );
}
