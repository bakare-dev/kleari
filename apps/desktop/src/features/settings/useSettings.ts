import { useCallback, useEffect, useRef, useState } from "react";
import type { NotificationAccess, Preferences, ScanSchedule, SettingsState, Uninstalled } from "../../contracts/settings";
import type { FolderSource } from "../../contracts/settings";
import { quitKleari } from "../../lib/menubarIpc";
import {
  addExcludedFolder,
  getSettings,
  removeExcludedFolder,
  removeFolderAccess,
  requestNotifications,
  revealQuarantineFolder,
  setOpenAtLogin,
  setPreferences,
  uninstallKleari,
} from "../../lib/settingsIpc";
import { errorKind } from "../cleanup/useCleanupScan";
import { storageErrorText } from "../storage/format";

export interface SettingsLoaders {
  get: () => Promise<SettingsState>;
  save: typeof setPreferences;
  openAtLogin: typeof setOpenAtLogin;
  requestNotifications: typeof requestNotifications;
  addExclusion: typeof addExcludedFolder;
  removeExclusion: typeof removeExcludedFolder;
  removeFolder: typeof removeFolderAccess;
  revealQuarantine: typeof revealQuarantineFolder;
  uninstall: typeof uninstallKleari;
  quit: typeof quitKleari;
}

const defaultLoaders: SettingsLoaders = {
  get: getSettings,
  save: setPreferences,
  openAtLogin: setOpenAtLogin,
  requestNotifications,
  addExclusion: addExcludedFolder,
  removeExclusion: removeExcludedFolder,
  removeFolder: removeFolderAccess,
  revealQuarantine: revealQuarantineFolder,
  uninstall: uninstallKleari,
  quit: quitKleari,
};

/** How an uninstall went. `held`: quarantine still holds items, so nothing was moved. */
export type UninstallOutcome = { kind: "done"; uninstalled: Uninstalled } | { kind: "held"; message: string } | { kind: "failed"; message: string };

/** The notification preferences that need macOS's permission to be turned on. */
export type NotificationKey = "notifyScan" | "notifyQuarantine" | "notifyIssues";

/** The outcome of a change: `null` when it took effect, otherwise what to tell the user. */
export type ChangeResult = string | null;

/**
 * The Settings page's state, read at launch and on each visit. A preference applies as soon
 * as it is saved; if saving fails the last saved value is shown again, with the reason.
 * `onPreferences` runs with every confirmed preferences (so Cleanup can start from them).
 */
export function useSettings(onPreferences: (preferences: Preferences) => void, loaders: SettingsLoaders = defaultLoaders) {
  const [state, setState] = useState<SettingsState | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The last save failed; cleared by the next one that succeeds.
  const [saveFailed, setSaveFailed] = useState(false);
  const mounted = useRef(true);
  const applied = useRef(onPreferences);
  // Every read and change takes a ticket; a response is shown only if no later one was
  // started, so an older read never overwrites newer state.
  const ticket = useRef(0);
  // What the backend last confirmed: shown again when a save fails.
  const confirmed = useRef<SettingsState | null>(null);
  // Saves run one at a time, each from the state the previous one left.
  const saving = useRef<Promise<unknown>>(Promise.resolve());
  // Cleanup takes the saved defaults once at launch, then after each save.
  const defaultsApplied = useRef(false);
  useEffect(() => {
    applied.current = onPreferences;
  }, [onPreferences]);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const show = useCallback((next: SettingsState) => {
    confirmed.current = next;
    setState(next);
  }, []);

  const refresh = useCallback(() => {
    const mine = ++ticket.current;
    // After any save under way, so the read sees what it saved.
    const settled = () => loaders.get();
    saving.current.then(settled, settled).then(
      (next) => {
        if (!mounted.current || mine !== ticket.current) {
          // A later read or change is under way; it decides what is shown.
          return;
        }
        show(next);
        setError(null);
        if (!defaultsApplied.current) {
          defaultsApplied.current = true;
          applied.current(next.preferences.preferences);
        }
      },
      (failure: unknown) => {
        if (mounted.current && mine === ticket.current) {
          setError(storageErrorText[errorKind(failure)]);
        }
      },
    );
  }, [loaders, show]);

  // Read at launch so Cleanup starts from the saved defaults.
  useEffect(() => {
    refresh();
  }, [refresh]);

  /** Updates what was confirmed (a change the backend reported) and shows it if still current. */
  const confirm = useCallback(
    (mine: number, change: (current: SettingsState) => SettingsState) => {
      const base = confirmed.current;
      if (!base || !mounted.current) {
        return;
      }
      confirmed.current = change(base);
      if (mine === ticket.current) {
        setState(confirmed.current);
      }
    },
    [],
  );

  const save = useCallback(
    (next: Preferences): Promise<ChangeResult> => {
      const run = async (): Promise<ChangeResult> => {
        if (!confirmed.current) {
          return "Settings haven’t loaded yet.";
        }
        const mine = ++ticket.current;
        setState({ ...confirmed.current, preferences: { preferences: next, source: "saved" } });
        try {
          const saved = await loaders.save(next);
          confirm(mine, (current) => ({ ...current, preferences: saved }));
          applied.current(saved.preferences);
          if (mounted.current) {
            setSaveFailed(false);
          }
          return null;
        } catch (failure) {
          if (mounted.current && mine === ticket.current && confirmed.current) {
            setState(confirmed.current);
          }
          if (mounted.current) {
            setSaveFailed(true);
          }
          return `Not saved: ${storageErrorText[errorKind(failure)]}`;
        }
      };
      const result = saving.current.then(run, run);
      saving.current = result;
      return result;
    },
    [loaders, confirm],
  );

  const openAtLogin = useCallback(
    async (enabled: boolean): Promise<ChangeResult> => {
      const mine = ++ticket.current;
      try {
        const change = await loaders.openAtLogin(enabled);
        const now = change.state;
        confirm(mine, (current) => ({ ...current, loginItem: now }));
        if (now === "requiresApproval") {
          return "Allow Kleari in System Settings › General › Login Items to finish.";
        }
        if (now === "unavailable") {
          return "macOS didn’t recognise this copy of Kleari. Move it to Applications, open it from there, and try again.";
        }
        if (now === "unsupported") {
          return "Open at login needs macOS 13 or later.";
        }
        if ((now === "enabled") === enabled) {
          return null;
        }
        // macOS's own reason, when it gave one.
        return change.refusal
          ? `macOS didn’t make the change: ${change.refusal}`
          : "macOS didn’t make the change. Try again in System Settings › General › Login Items.";
      } catch (failure) {
        return storageErrorText[errorKind(failure)];
      }
    },
    [loaders, confirm],
  );

  /** Turns a notification on (asking macOS first, if it hasn't been asked) or off. */
  const notify = useCallback(
    async (key: NotificationKey, on: boolean): Promise<ChangeResult> => {
      if (!confirmed.current) {
        return "Settings haven’t loaded yet.";
      }
      let access: NotificationAccess = confirmed.current.notifications;
      if (on && access !== "allowed") {
        const mine = ++ticket.current;
        try {
          access = await loaders.requestNotifications();
        } catch (failure) {
          return storageErrorText[errorKind(failure)];
        }
        const answer = access;
        confirm(mine, (current) => ({ ...current, notifications: answer }));
        if (access !== "allowed") {
          return access === "denied"
            ? "macOS isn’t allowing Kleari’s notifications. Turn them on in System Settings › Notifications."
            : "macOS didn’t recognise this copy of Kleari for notifications. Move it to Applications, open it from there, and try again.";
        }
      }
      const base = confirmed.current;
      return base ? save({ ...base.preferences.preferences, [key]: on }) : "Settings haven’t loaded yet.";
    },
    [loaders, save, confirm],
  );

  /**
   * Saves the scan schedule, then reads Settings again for its next time. Turning it on asks
   * macOS to allow notifications first if it hasn't been asked; refused, the schedule still
   * runs and its findings wait in the review.
   */
  const schedule = useCallback(
    async (next: ScanSchedule): Promise<ChangeResult> => {
      const base = confirmed.current;
      if (!base) {
        return "Settings haven’t loaded yet.";
      }
      let access: NotificationAccess = base.notifications;
      if (next.frequency !== "off" && access === "notAsked") {
        const mine = ++ticket.current;
        try {
          access = await loaders.requestNotifications();
          const answer = access;
          confirm(mine, (current) => ({ ...current, notifications: answer }));
        } catch (failure) {
          return storageErrorText[errorKind(failure)];
        }
      }
      const current = confirmed.current ?? base;
      const saved = await save({ ...current.preferences.preferences, scanSchedule: next });
      if (saved !== null) {
        return saved;
      }
      refresh();
      return next.frequency !== "off" && access !== "allowed"
        ? "Scheduled. macOS isn’t allowing Kleari’s notifications, so its findings will wait in Cleanup’s review."
        : null;
    },
    [loaders, save, confirm, refresh],
  );

  /** Runs a folder change and shows the list the backend returns. */
  const folderChange = useCallback(
    async <T,>(request: () => Promise<T>, change: (current: SettingsState, result: T) => SettingsState): Promise<ChangeResult> => {
      const mine = ++ticket.current;
      try {
        const result = await request();
        confirm(mine, (current) => change(current, result));
        return null;
      } catch (failure) {
        if (mounted.current && mine === ticket.current && confirmed.current) {
          setState(confirmed.current);
        }
        return storageErrorText[errorKind(failure)];
      }
    },
    [confirm],
  );

  const addExclusion = useCallback(
    () => folderChange(loaders.addExclusion, (current, exclusions) => (exclusions ? { ...current, exclusions } : current)),
    [folderChange, loaders],
  );

  const removeExclusion = useCallback(
    (path: string) => folderChange(() => loaders.removeExclusion(path), (current, exclusions) => ({ ...current, exclusions })),
    [folderChange, loaders],
  );

  const removeFolder = useCallback(
    (source: FolderSource, path: string) => folderChange(() => loaders.removeFolder(source, path), (current, folders) => ({ ...current, folders })),
    [folderChange, loaders],
  );

  const revealQuarantine = useCallback(async (): Promise<ChangeResult> => {
    try {
      await loaders.revealQuarantine();
      return null;
    } catch (failure) {
      return storageErrorText[errorKind(failure)];
    }
  }, [loaders]);

  const uninstall = useCallback(async (): Promise<UninstallOutcome> => {
    try {
      return { kind: "done", uninstalled: await loaders.uninstall() };
    } catch (failure) {
      const kind = errorKind(failure);
      return { kind: kind === "quarantineNotEmpty" ? "held" : "failed", message: storageErrorText[kind] };
    }
  }, [loaders]);

  /** Quits Kleari; resolves with what to tell the user only if it didn't. */
  const quit = useCallback(async (): Promise<ChangeResult> => {
    try {
      await loaders.quit();
      return null;
    } catch (failure) {
      return storageErrorText[errorKind(failure)];
    }
  }, [loaders]);

  return { state, error, saveFailed, refresh, save, openAtLogin, notify, schedule, addExclusion, removeExclusion, removeFolder, revealQuarantine, uninstall, quit };
}

export type Settings = ReturnType<typeof useSettings>;
