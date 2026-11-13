import { useEffect, useRef, useState, type ReactNode } from "react";
import { Glyph, type IconName } from "../../ui/icons";
import { Button, IconTile, Modal, PageHeader, ScrollBody, Segmented, Select, Stepper, Switch, Toast, useToast, type Status, type Tone } from "../../ui/components";
import { detailFromHash, headingIdFor } from "../../shell/routes";
import type { AppearancePreference, AppearanceStorageStatus } from "../../../features/settings/appearance";
import type { AccessFolder, FolderSource, LoginItem, NotificationAccess, Preferences, ScanSchedule, ScheduleFrequency, ScheduleStatus, SettingsState, Uninstalled } from "../../../contracts/settings";
import type { ScanMode } from "../../../contracts/cleanup";
import type { UninstallOutcome } from "../../../features/settings/useSettings";
import type { UpdateState } from "../../../features/settings/useUpdates";
import { installable, updateStatus } from "./Updates";
import "./settings.css";

/** Kleari's bundle identifier (tauri.conf.json), which macOS keys its permissions by. */
export const BUNDLE_ID = "dev.bakare.kleari";
const RESET_PERMISSIONS = `tccutil reset All ${BUNDLE_ID}`;

type Dialog =
  | { kind: "access"; folder: AccessFolder }
  | { kind: "log" }
  | { kind: "uninstall"; working: boolean; problem?: string }
  | { kind: "held"; message: string }
  | { kind: "uninstalled"; uninstalled: Uninstalled; quitProblem?: string };

type Section = "general" | "scanning" | "access" | "quarantine" | "notifications" | "privacy" | "about";

/** What a change came to: `null` when it took effect, otherwise what to tell the user. */
type Change = Promise<string | null>;

interface SettingsScreenProps {
  appearance: AppearancePreference;
  appearanceStatus: AppearanceStorageStatus;
  windowThemeFailed: boolean;
  onAppearanceChange: (preference: AppearancePreference) => void;
  /** `null` while the first read is under way. */
  state: SettingsState | null;
  /** Why the settings couldn't be read; the last read values stay shown. */
  error: string | null;
  /** The last change couldn't be saved (until one succeeds). */
  saveFailed?: boolean;
  onSave: (preferences: Preferences) => Change;
  onOpenAtLogin: (enabled: boolean) => Change;
  onNotify: (key: "notifyScan" | "notifyQuarantine", on: boolean) => Change;
  onSchedule: (schedule: ScanSchedule) => Change;
  onAddExclusion: () => Change;
  onRemoveExclusion: (path: string) => Change;
  onRemoveFolder: (source: FolderSource, path: string) => Change;
  onRevealQuarantine: () => Change;
  onClearLog: () => Change;
  updates: UpdateState;
  onCheckUpdates: () => void;
  onInstallUpdate: () => void;
  /** Moves Kleari and its data to the Trash (refused while quarantine holds items). */
  onUninstall: () => Promise<UninstallOutcome>;
  onQuit: () => Change;
  onOpenQuarantine: () => void;
  /** Grant access and Add folder live on the Cleanup page. */
  onOpenCleanup: () => void;
  onOpenPane: (pane: "loginItems" | "fullDiskAccess" | "filesAndFolders" | "notifications") => void;
}

const sections: readonly { id: Section; label: string; blurb: string; tone: Tone; icon: IconName }[] = [
  { id: "general", label: "General", blurb: "Startup, appearance and the default scan", tone: "gray", icon: "gear" },
  { id: "scanning", label: "Scanning", blurb: "What Kleari leaves out of its findings", tone: "blue", icon: "search" },
  { id: "access", label: "Folders and access", blurb: "Folders Kleari may read", tone: "teal", icon: "folder" },
  { id: "quarantine", label: "Quarantine", blurb: "Where moved items wait until you decide", tone: "orange", icon: "quarantine" },
  { id: "notifications", label: "Notifications", blurb: "When Kleari lets you know", tone: "red", icon: "bell" },
  { id: "privacy", label: "Privacy and safety", blurb: "What stays on this Mac, the activity log and built-in rules", tone: "green", icon: "shield" },
  { id: "about", label: "About", blurb: "Version, updates and uninstalling", tone: "indigo", icon: "info" },
];

const folderSourceText: Record<FolderSource, string> = {
  protected: "Read only · you chose Grant access",
  cleanup: "Read only · added in Cleanup for this session",
  developer: "Read only · added in Developer for this session",
  restore: "Restores only · chosen in Quarantine for this session",
};

const loginItemNote: Partial<Record<LoginItem, string>> = {
  requiresApproval: "Waiting for your approval in System Settings › General › Login Items.",
  unavailable: "macOS doesn’t recognise this copy of Kleari as an app it can open at login. Move Kleari to Applications, open it from there, and try again.",
  unsupported: "Open at login needs macOS 13 or later. You can add Kleari yourself under Users & Groups › Login Items.",
};

const notificationNote: Record<NotificationAccess, string | null> = {
  allowed: null,
  notAsked: "macOS asks once, the first time you turn one on.",
  denied: "macOS isn’t allowing Kleari’s notifications.",
  unavailable: "macOS doesn’t recognise this copy of Kleari for notifications. Move Kleari to Applications, open it from there, and try again.",
};

export function Row({ label, sub, mono, locked, control }: { label: string; sub?: string; mono?: string; locked?: boolean; control: ReactNode }) {
  return (
    <div className="set-row">
      <span className="v2-row-text">
        <span className="set-row-label">
          {label}
          {locked && <span className="set-lock" role="img" aria-label="Built in"><Glyph name="lock" size={12} strokeWidth={2.4} /></span>}
        </span>
        {sub && <span className="set-row-sub">{sub}</span>}
        {mono && <span className="set-row-mono v2-mono v2-ellipsis">{mono}</span>}
      </span>
      {control}
    </div>
  );
}

export function Group({ title, note, children }: { title: string; note?: ReactNode; children: ReactNode }) {
  return (
    <section aria-label={title} className="set-group">
      <h3>{title}</h3>
      <div className="v2-card set-card">{children}</div>
      {note && <span className="set-note">{note}</span>}
    </section>
  );
}

function Badge({ status, children }: { status: Status; children: ReactNode }) {
  return <span className={`v2-pill v2-pill-${status} set-badge`}>{children}</span>;
}

export const appearanceNotes: Partial<Record<AppearanceStorageStatus, string>> = {
  unreadable: "Your saved appearance couldn’t be read, so Kleari is using System.",
  invalid: "The saved appearance wasn’t recognised, so Kleari is using System.",
  saveFailed: "This choice applies now but couldn’t be saved, so Kleari will use System next time.",
};

/** The section a Settings link names, or General. */
export function sectionFromHash(hash: string): Section {
  const detail = detailFromHash(hash);
  return sections.find((entry) => entry.id === detail)?.id ?? "general";
}

const weekdays = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"] as const;
const hourOptions = Array.from({ length: 24 }, (_, hour) => ({ value: String(hour), label: `${String(hour).padStart(2, "0")}:00` }));
const weekdayOptions = weekdays.map((day, index) => ({ value: String(index), label: day }));

/** "Tue 09:00", in this Mac's time zone. */
function scheduleTime(ms: number): string {
  const date = new Date(ms);
  return `${date.toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" })}, ${date.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit" })}`;
}

/** Where the schedule stands, under its controls. */
export function scheduleNote(schedule: ScanSchedule | undefined, status: ScheduleStatus | undefined): string {
  const runs = "Scheduled scans only read, and run while Kleari is open or in the menu bar; Open at login keeps them going after a restart. A time missed meanwhile runs once when Kleari is next open.";
  if (!schedule || schedule.frequency === "off" || !status) {
    return runs;
  }
  const next = status.nextMs === null ? "" : `Next scan: ${scheduleTime(status.nextMs)}. `;
  const last = status.lastScanMs === null ? "" : `Last scheduled scan: ${scheduleTime(status.lastScanMs)}. `;
  return `${next}${last}${runs}`;
}

/** The next value a stepper offers: within `low`–`high`, in steps, with `extra` (a year) allowed as the last stop. */
function stepped(value: number, by: number, low: number, high: number, extra?: number): number {
  if (extra !== undefined && value === extra) {
    return by < 0 ? high : extra;
  }
  const next = value + by;
  if (extra !== undefined && next > high) {
    return extra;
  }
  return Math.min(high, Math.max(low, next));
}

/**
 * Uninstall, in steps: confirm; refused while quarantine holds items (with a way there); then
 * what went to the Trash, the one step macOS leaves to you (its permissions), and Quit.
 */
export function UninstallDialog(props: {
  dialog: Extract<Dialog, { kind: "uninstall" | "held" | "uninstalled" }>;
  onCancel: () => void;
  onConfirm: () => void;
  onOpenQuarantine: () => void;
  onQuit: () => void;
}) {
  const { dialog } = props;
  if (dialog.kind === "held") {
    return (
      <div className="v2-dialog-body v2-dialog-centered">
        <IconTile tone="orange" icon="quarantine" size={52} glow />
        <h2 id="set-dialog-title" className="v2-dialog-title">Empty quarantine first</h2>
        <p className="v2-dialog-text">{dialog.message} Nothing was moved.</p>
        <div className="v2-dialog-actions">
          <Button variant="secondary" onClick={props.onCancel}>Cancel</Button>
          <Button variant="primary" onClick={props.onOpenQuarantine}>Open Quarantine</Button>
        </div>
      </div>
    );
  }
  if (dialog.kind === "uninstalled") {
    const { moved, failed, loginItemKept } = dialog.uninstalled;
    const partly = failed.length > 0 || loginItemKept !== null;
    return (
      <div className="v2-dialog-body v2-dialog-centered">
        <IconTile tone={partly ? "yellow" : "green"} icon={partly ? "warning" : "check"} size={52} glow />
        <h2 id="set-dialog-title" className="v2-dialog-title">{partly ? "Partly uninstalled" : "Kleari is in the Trash"}</h2>
        <p className="v2-dialog-text">
          {moved.length > 0 ? `Moved to the Trash: ${moved.join(", ")}.` : "There was nothing to move."}
          {failed.length > 0 && ` Couldn’t be moved, so remove them yourself: ${failed.join(", ")}.`}
        </p>
        {loginItemKept !== null && (
          <p className="v2-dialog-text">
            Open at login is still on ({loginItemKept}). Remove Kleari in System Settings › General › Login Items.
          </p>
        )}
        <p className="v2-dialog-text">
          macOS keeps the permissions you gave Kleari until you clear them. In Terminal, run:
        </p>
        <code className="set-command v2-mono">{RESET_PERMISSIONS}</code>
        {dialog.quitProblem && <p role="alert" className="v2-dialog-text">{dialog.quitProblem} Quit with ⌘Q.</p>}
        <div className="v2-dialog-actions">
          <Button variant="primary" onClick={props.onQuit}>Quit Kleari</Button>
        </div>
      </div>
    );
  }
  return (
    <div className="v2-dialog-body v2-dialog-centered">
      <IconTile tone="red" icon="trash" size={52} glow />
      <h2 id="set-dialog-title" className="v2-dialog-title">Uninstall Kleari?</h2>
      <p className="v2-dialog-text">
        Kleari tries to turn off Open at login (and tells you if macOS keeps it on), then moves itself, its settings, activity history and caches to the Trash, and asks you to quit. Your own files stay where they are. You can put it back from the Trash until you empty it.
      </p>
      {dialog.problem && <p role="alert" className="v2-dialog-text">{dialog.problem}</p>}
      <div className="v2-dialog-actions">
        <Button variant="secondary" disabled={dialog.working} onClick={props.onCancel}>Cancel</Button>
        <Button variant="danger" disabled={dialog.working} onClick={props.onConfirm}>{dialog.working ? "Uninstalling…" : "Uninstall"}</Button>
      </div>
    </div>
  );
}

export function SettingsScreen(props: SettingsScreenProps) {
  const { state } = props;
  // A link such as `#settings/scanning` (Cleanup's "Schedule a scan…") opens that section.
  const [section, setSection] = useState<Section>(() => sectionFromHash(typeof window === "undefined" ? "" : window.location.hash));
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [savedFlash, setSavedFlash] = useState(false);
  const flashTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const toast = useToast();
  useEffect(() => () => clearTimeout(flashTimer.current), []);

  const flash = () => {
    clearTimeout(flashTimer.current);
    setSavedFlash(true);
    flashTimer.current = setTimeout(() => setSavedFlash(false), 1400);
  };
  /** Shows "Saved" when the change took effect, otherwise says why not. */
  const after = (change: Change, done?: string) => {
    void change.then((message) => {
      if (message) {
        toast.show(message, "warn");
      } else {
        flash();
        if (done) {
          toast.show(done);
        }
      }
    });
  };

  const current = sections.find((entry) => entry.id === section) ?? sections[0];
  const appearanceNote = props.windowThemeFailed
    ? "The window title bar couldn’t switch to match. The rest of Kleari uses your choice."
    : appearanceNotes[props.appearanceStatus];
  const preferences = state?.preferences.preferences;
  // A beta build gets betas until Beta updates is chosen (the same rule as Rust's `gets_betas`).
  const installedIsBeta = state?.version.includes("-") ?? false;
  const update = installable(props.updates);
  const set =<K extends keyof Preferences>(key: K, value: Preferences[K]) => preferences && after(props.onSave({ ...preferences, [key]: value }));
  const stepper = (key: "minSizeMb" | "skipRecentDays" | "quarantineReminderGb" | "retentionDays", label: string, low: number, high: number, step: number, unit: string, extra?: number) => {
    const value = preferences?.[key] ?? low;
    const shown = value === 0 && low === 0 ? "Off" : extra !== undefined && value === extra ? "1 year" : `${value} ${unit}`;
    return (
      <Stepper
        label={label}
        value={shown}
        canDecrease={preferences !== undefined && value > low}
        canIncrease={preferences !== undefined && value < (extra ?? high)}
        onDecrease={() => set(key, stepped(value, -step, low, high, extra))}
        onIncrease={() => set(key, stepped(value, step, low, high, extra))}
      />
    );
  };
  // Saving works only once settings were read and the history can be written.
  const canSave = state !== null && !props.error && state.preferences.source !== "unavailable" && !props.saveFailed;
  const preferencesNote =
    state?.preferences.source === "invalid"
      ? "Your saved settings couldn’t be used, so Kleari is using the defaults. Changing one saves them again."
      : state?.preferences.source === "unavailable"
        ? "Kleari’s history file can’t be read, so the defaults apply and changes can’t be saved."
        : null;

  return (
    <>
      <PageHeader
        title="Settings"
        headingId={headingIdFor("settings")}
        subtitle="Changes save as you make them. Safety rules are built in and can’t be turned off."
        actions={
          // Claimed only after a confirmed read, while changes can be saved.
          canSave ? (
            <span role="status" className={`set-saved${savedFlash ? " on" : ""}`}>
              <Glyph name="check" size={13} strokeWidth={2.6} />
              {savedFlash ? "Saved" : "Saved automatically"}
            </span>
          ) : (
            <span role="status" className="set-saved">
              {state === null && !props.error
                ? "Reading settings…"
                : props.saveFailed
                  ? "Your last change wasn’t saved"
                  : "Changes can’t be saved right now"}
            </span>
          )
        }
      />
      <div className="set-body">
        <nav aria-label="Settings sections" className="set-sections">
          {sections.map((entry) => (
            <button key={entry.id} type="button" aria-current={entry.id === section ? "page" : undefined} className="set-section" onClick={() => setSection(entry.id)}>
              <IconTile tone={entry.tone} icon={entry.icon} size={24} />
              <span>{entry.label}</span>
            </button>
          ))}
        </nav>
        <ScrollBody label={current.label} className="set-content">
          <div className="set-content-head">
            <IconTile tone={current.tone} icon={current.icon} size={44} glow />
            <span className="v2-row-text">
              <h2>{current.label}</h2>
              <span className="v2-muted">{current.blurb}</span>
            </span>
          </div>
          {(props.error || preferencesNote) && (
            <div role="alert" className="v2-banner v2-banner-warn set-banner">
              <IconTile tone="yellow" icon="warning" size={32} />
              <div className="v2-banner-text">
                <strong>{props.error ? "Settings couldn’t be read" : "Using the defaults"}</strong>
                <span>{props.error ?? preferencesNote}</span>
              </div>
            </div>
          )}

          {section === "general" && (
            <>
              <Group title="Startup" note={state ? loginItemNote[state.loginItem] : undefined}>
                <Row
                  label="Open at login"
                  sub="Kleari opens when you log in. macOS lists it in Login Items, where you can turn it off too."
                  control={
                    <Switch
                      label="Open at login"
                      checked={state?.loginItem === "enabled" || state?.loginItem === "requiresApproval"}
                      disabled={!state || state.loginItem === "unavailable" || state.loginItem === "unsupported"}
                      onChange={(on) => after(props.onOpenAtLogin(on))}
                    />
                  }
                />
                {state?.loginItem === "requiresApproval" && (
                  <Row label="Approve in System Settings" sub="General › Login Items" control={<Button variant="soft" size="sm" onClick={() => props.onOpenPane("loginItems")}>Open</Button>} />
                )}
                <Row
                  label="Show in menu bar"
                  sub="Live readings a click away. Closing the window keeps Kleari running there; quit from the menu bar or with ⌘Q."
                  control={<Switch label="Show in menu bar" checked={preferences?.menuBar ?? true} disabled={!preferences} onChange={(on) => set("menuBar", on)} />}
                />
              </Group>
              <Group title="Look" note={appearanceNote}>
                <Row
                  label="Appearance"
                  sub="Match macOS or pick one"
                  control={
                    <Segmented
                      label="Appearance"
                      kind="radio"
                      value={props.appearance}
                      onChange={(value) => { props.onAppearanceChange(value); flash(); }}
                      options={[{ value: "system", label: "System" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]}
                    />
                  }
                />
                <Row
                  label="Default scan"
                  sub="Cleanup starts with it selected; you can still pick another"
                  control={
                    <Segmented<ScanMode>
                      label="Default scan"
                      kind="radio"
                      value={preferences?.defaultScan ?? "standard"}
                      onChange={(value) => set("defaultScan", value)}
                      options={[{ value: "standard", label: "Standard" }, { value: "deep", label: "Deep" }, { value: "dev", label: "Developer" }]}
                    />
                  }
                />
              </Group>
            </>
          )}

          {section === "scanning" && (
            <>
              <Group title="What to look at" note="These only leave findings out. Nothing becomes eligible to move because of them.">
                <Row
                  label="Include hidden files"
                  sub="Private dotfiles such as .env and .ssh, in a Deep scan. Build and cache folders such as .next are always searched"
                  control={<Switch label="Include hidden files" checked={preferences?.scanDefaults.hidden ?? false} disabled={!preferences} onChange={(on) => preferences && set("scanDefaults", { ...preferences.scanDefaults, hidden: on })} />}
                />
                <Row label="Skip large files changed in the last" sub="Recently changed large files are never suggested" control={stepper("skipRecentDays", "recent-file window", 0, 90, 1, "days")} />
                <Row label="Ignore items smaller than" sub="Keeps results short" control={stepper("minSizeMb", "minimum size", 0, 500, 10, "MB")} />
              </Group>
              <Group title="Schedule" note={scheduleNote(preferences?.scanSchedule, state?.schedule)}>
                <Row
                  label="Scan automatically"
                  sub="Notifies what it found; click the notification to review. Nothing moves until you approve it."
                  control={
                    <Segmented<ScheduleFrequency>
                      label="Scan automatically"
                      kind="radio"
                      value={preferences?.scanSchedule.frequency ?? "off"}
                      onChange={(frequency) => preferences && after(props.onSchedule({ ...preferences.scanSchedule, frequency }))}
                      options={[{ value: "off", label: "Off" }, { value: "daily", label: "Daily" }, { value: "weekly", label: "Weekly" }]}
                    />
                  }
                />
                {preferences && preferences.scanSchedule.frequency !== "off" && (
                  <>
                    {preferences.scanSchedule.frequency === "weekly" && (
                      <Row
                        label="Day"
                        control={<Select label="Day" value={String(preferences.scanSchedule.weekday)} options={weekdayOptions} onChange={(day) => after(props.onSchedule({ ...preferences.scanSchedule, weekday: Number(day) }))} />}
                      />
                    )}
                    <Row
                      label="Time"
                      sub="This Mac’s time"
                      control={<Select label="Time" value={String(preferences.scanSchedule.hour)} options={hourOptions} onChange={(hour) => after(props.onSchedule({ ...preferences.scanSchedule, hour: Number(hour) }))} />}
                    />
                    <Row
                      label="Scan"
                      sub="With the scan options you set in Cleanup"
                      control={
                        <Segmented<ScanMode>
                          label="Scheduled scan"
                          kind="radio"
                          value={preferences.scanSchedule.mode}
                          onChange={(mode) => after(props.onSchedule({ ...preferences.scanSchedule, mode }))}
                          options={[{ value: "standard", label: "Standard" }, { value: "deep", label: "Deep" }, { value: "dev", label: "Developer" }]}
                        />
                      }
                    />
                  </>
                )}
              </Group>
              <Group title="Never scan these" note="Cleanup and Developer scans never open them, nor offer a folder that contains one, and they’re left out of Storage’s large files. Storage’s totals still measure them. Anything already in the review from them is no longer offered to move.">

                {(state?.exclusions ?? []).map((path) => (
                  <Row key={path} label={path.split("/").pop() || path} mono={path} control={<Button variant="soft" size="sm" onClick={() => after(props.onRemoveExclusion(path))}>Remove</Button>} />
                ))}
                <Row label="Add a folder to skip" sub="You’ll pick it in a macOS window" control={<Button variant="soft" size="sm" disabled={!state} onClick={() => after(props.onAddExclusion())}>Add folder…</Button>} />
              </Group>
            </>
          )}

          {section === "access" && (
            <>
              <Group title="Allowed folders" note="Kleari reads only the folders listed here, besides the fixed places each page names. Removing one takes effect right away.">
                {(state?.folders ?? []).map((folder) => (
                  <Row
                    key={`${folder.source}${folder.path}`}
                    label={folder.name}
                    sub={folderSourceText[folder.source]}
                    mono={folder.path}
                    control={<Button variant="dangerOutline" size="sm" className="set-danger" onClick={() => setDialog({ kind: "access", folder })}>Remove access</Button>}
                  />
                ))}
                {state && state.folders.length === 0 && <Row label="No folders yet" sub="Grant access to Downloads, Desktop, Documents or Movies, or add a folder, on the Cleanup page" control={null} />}
                <Row label="Allow another folder" sub="Grant access or add a folder on the Cleanup page" control={<Button variant="soft" size="sm" onClick={props.onOpenCleanup}>Open Cleanup</Button>} />
              </Group>
              <Group title="Full Disk Access">
                <Row
                  label="Full Disk Access"
                  sub="Not needed for normal scans. With it, protected folders and private caches can be read without asking."
                  control={state ? <Badge status={state.fullDiskAccess ? "ok" : "muted"}>{state.fullDiskAccess ? "Granted" : "Not granted"}</Badge> : null}
                />
                <Row label="Change in System Settings" sub="Privacy and Security › Full Disk Access" control={<Button variant="soft" size="sm" onClick={() => props.onOpenPane("fullDiskAccess")}>Open</Button>} />
              </Group>
            </>
          )}

          {section === "quarantine" && (
            <>
              <Group title="Storage">
                <Row
                  label="Quarantine folder"
                  sub="On this Mac, so items still use space until you decide"
                  mono={state?.quarantineFolder}
                  control={<Button variant="soft" size="sm" disabled={!state} onClick={() => after(props.onRevealQuarantine())}>Show in Finder</Button>}
                />
                <Row label="Remind me when quarantine passes" sub="A notification, if turned on. Nothing is deleted." control={stepper("quarantineReminderGb", "reminder size", 0, 100, 1, "GB")} />
              </Group>
              <Group title="Built-in rules" note="These protect your files and can’t be turned off.">
                <Row locked label="Keep items until you decide" sub="Kleari never empties quarantine on its own" control={<Badge status="ok">Always on</Badge>} />
                <Row locked label="Confirm every permanent delete" sub="You see the list and tick “I understand” each time" control={<Badge status="ok">Always on</Badge>} />
                <Row locked label="Never overwrite on restore" sub="If a file is already there, the item stays in quarantine" control={<Badge status="ok">Always on</Badge>} />
              </Group>
            </>
          )}

          {section === "notifications" && (
            <>
              <Group title="Notify me when" note={state ? notificationNote[state.notifications] : undefined}>
                <Row label="A Cleanup scan finishes" sub="With what it found; nothing is changed" control={<Switch label="A Cleanup scan finishes" checked={preferences?.notifyScan ?? false} disabled={!state} onChange={(on) => after(props.onNotify("notifyScan", on))} />} />
                <Row label="Quarantine gets large" sub="Set the size in Quarantine" control={<Switch label="Quarantine gets large" checked={preferences?.notifyQuarantine ?? false} disabled={!state} onChange={(on) => after(props.onNotify("notifyQuarantine", on))} />} />
                {state?.notifications === "denied" && (
                  <Row label="Allow in System Settings" sub="Notifications › Kleari" control={<Button variant="soft" size="sm" onClick={() => props.onOpenPane("notifications")}>Open</Button>} />
                )}
              </Group>
              <Group title="Style">
                <Row label="Play a sound" sub="Uses your macOS alert sound" control={<Switch label="Play a sound" checked={preferences?.notifySound ?? false} disabled={!preferences} onChange={(on) => set("notifySound", on)} />} />
              </Group>
            </>
          )}

          {section === "privacy" && (
            <>
              <Group title="On this Mac">
                <Row
                  locked
                  label="Your files stay on this Mac"
                  sub="Nothing about your files or this Mac is sent anywhere. The one internet request is the update check (About), which only asks GitHub for Kleari’s latest release. No telemetry, account or cloud."
                  control={<Badge status="ok">Always on</Badge>}
                />
              </Group>
              <Group title="Activity log" note="The log is kept on this Mac only. Clearing it or keeping less doesn’t touch your files or quarantine.">
                <Row label="Keep activity for" sub="Older entries are deleted" control={stepper("retentionDays", "activity history", 30, 360, 30, "days", 365)} />
                <Row label="Clear activity log" sub="Your files and quarantine aren’t affected" control={<Button variant="dangerOutline" size="sm" className="set-danger" disabled={!state} onClick={() => setDialog({ kind: "log" })}>Clear log…</Button>} />
              </Group>
            </>
          )}

          {section === "about" && (
            <>
              <Group title="Kleari">
                <Row label="Version" sub="This copy of Kleari" control={state ? <Badge status="link">{state.version}</Badge> : null} />
              </Group>
              <Group
                title="Updates"
                note="Checking asks GitHub for Kleari’s latest release. Nothing about this Mac or your files is sent. An update installs only when you choose it, and only if it carries Kleari’s signature."
              >
                <Row
                  label="Check for updates automatically"
                  sub="Once, each time Kleari opens"
                  control={<Switch label="Check for updates automatically" checked={preferences?.checkForUpdates ?? true} disabled={!preferences} onChange={(on) => set("checkForUpdates", on)} />}
                />
                <Row
                  label="Beta updates"
                  sub={
                    preferences?.betaUpdates === null && installedIsBeta
                      ? "On, because this copy is a beta. Turning this off keeps your version until a newer stable release is out."
                      : "Get new features early, in releases still being tested. Turning this off keeps your version until a newer stable release is out."
                  }
                  control={<Switch label="Beta updates" checked={preferences ? (preferences.betaUpdates ?? installedIsBeta) : false} disabled={!preferences} onChange={(on) => set("betaUpdates", on)} />}
                />
                <Row
                  label="Software update"
                  sub={updateStatus(props.updates)}
                  control={
                    update ? (
                      <Button variant="primary" size="sm" onClick={props.onInstallUpdate}>Install and restart</Button>
                    ) : (
                      <Button variant="soft" size="sm" disabled={props.updates.phase === "checking" || props.updates.phase === "installing"} onClick={props.onCheckUpdates}>
                        {props.updates.phase === "checking" ? "Checking…" : "Check now"}
                      </Button>
                    )
                  }
                />
                {update?.status === "available" && update.notes && <Row label="What’s new" sub={update.notes} control={null} />}
              </Group>
              <Group title="Uninstall" note="Your own files are never touched. Quarantine must be empty first, so nothing of yours goes to the Trash with Kleari.">
                <Row
                  label="Uninstall Kleari"
                  sub="Tries to turn off Open at login, then moves Kleari, its settings, history and caches to the Trash"
                  control={<Button variant="dangerOutline" size="sm" className="set-danger" onClick={() => setDialog({ kind: "uninstall", working: false })}>Uninstall…</Button>}
                />
              </Group>
            </>
          )}
          <div className="set-end" />
        </ScrollBody>
      </div>

      <Toast message={toast.message} />

      <Modal
        open={dialog !== null}
        // While it runs, and once Kleari is in the Trash, the only way on is the dialog's own button.
        onClose={() => !(dialog?.kind === "uninstalled" || (dialog?.kind === "uninstall" && dialog.working)) && setDialog(null)}
        labelledBy="set-dialog-title"
        role="alertdialog"
        className="set-dialog"
      >
        {dialog && (dialog.kind === "uninstall" || dialog.kind === "held" || dialog.kind === "uninstalled") && (
          <UninstallDialog
            dialog={dialog}
            onCancel={() => setDialog(null)}
            onConfirm={() => {
              setDialog({ kind: "uninstall", working: true });
              void props.onUninstall().then((outcome) =>
                setDialog(
                  outcome.kind === "done"
                    ? { kind: "uninstalled", uninstalled: outcome.uninstalled }
                    : outcome.kind === "held"
                      ? { kind: "held", message: outcome.message }
                      : { kind: "uninstall", working: false, problem: outcome.message },
                ),
              );
            }}
            onOpenQuarantine={() => {
              setDialog(null);
              props.onOpenQuarantine();
            }}
            onQuit={() => {
              void props.onQuit().then((problem) => problem && dialog.kind === "uninstalled" && setDialog({ ...dialog, quitProblem: problem }));
            }}
          />
        )}
        {dialog && (dialog.kind === "access" || dialog.kind === "log") && (
          <div className="v2-dialog-body v2-dialog-centered">
            <IconTile tone={dialog.kind === "access" ? "teal" : "red"} icon={dialog.kind === "access" ? "folder" : "trash"} size={52} glow />
            <h2 id="set-dialog-title" className="v2-dialog-title">{dialog.kind === "access" ? `Remove access to ${dialog.folder.name}?` : "Clear the activity log?"}</h2>
            <p className="v2-dialog-text">
              {dialog.kind === "access"
                ? dialog.folder.source === "protected"
                  ? `Kleari will stop reading ${dialog.folder.path} until you choose Grant access again. Nothing in the folder changes. macOS’s own permission is in System Settings › Files and Folders.`
                  : `Kleari will stop using ${dialog.folder.path} for this session. Nothing in the folder changes, and you can add it again any time.`
                : "This removes the record of past scans, moves and restores. Your files and quarantine stay exactly as they are."}
            </p>
            <div className="v2-dialog-actions">
              <Button variant="secondary" onClick={() => setDialog(null)}>Cancel</Button>
              <Button
                variant="danger"
                onClick={() => {
                  if (dialog.kind === "access") {
                    after(props.onRemoveFolder(dialog.folder.source, dialog.folder.path), `Removed access to ${dialog.folder.path}`);
                  } else {
                    after(props.onClearLog(), "Activity log cleared. Files and quarantine unchanged.");
                  }
                  setDialog(null);
                }}
              >
                {dialog.kind === "access" ? "Remove access" : "Clear log"}
              </Button>
            </div>
          </div>
        )}
      </Modal>
    </>
  );
}
