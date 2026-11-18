import { useCallback, useEffect, useState } from "react";
import type { Settings } from "../../features/settings/useSettings";
import type { MouseAccess } from "../../contracts/mouse";
import { DISCLAIMER_VERSION } from "../../features/legal/legal";
import { getMouse, requestMouseAccess } from "../../lib/mouseIpc";
import { dismissIdentityMove, requestNotifications } from "../../lib/settingsIpc";
import { openSystemSettings } from "../../lib/systemPageIpc";
import { AccessSteps } from "../ui/AccessSteps";
import { Button, IconTile, Modal, Pill, Switch } from "../ui/components";

/** The mouse settings that were on before the move, and whether macOS allows them now. */
interface MouseNow {
  enabled: boolean;
  access: MouseAccess;
}

/**
 * Shown once after Kleari moved to its new bundle identifier (dev.kleari.app): macOS gives a
 * new identifier no permissions, so this lists the ones Kleari used and how to allow each
 * again. Each is read again when Kleari's window comes back from System Settings. Done (or
 * Escape) closes it for good; Settings still offers every permission.
 */
export function IdentityMoveSheet({ settings }: { settings: Settings }) {
  const { state, refresh } = settings;
  const preferences = state?.preferences.preferences ?? null;
  const pending = state?.identityMove ?? null;
  const [open, setOpen] = useState(false);
  const [mouse, setMouse] = useState<MouseNow | null>(null);
  const [steps, setSteps] = useState<"fullDiskAccess" | "accessibility" | "notifications" | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    // After the "use at your own risk" notice and the first-launch setup, never on top of them.
    if (pending && preferences && preferences.disclaimerSeen >= DISCLAIMER_VERSION && preferences.setupDone) {
      setOpen(true);
    }
  }, [pending, preferences]);

  // Reading the Mouse settings also starts them once macOS allows Accessibility again.
  const readMouse = useCallback(() => {
    getMouse().then(
      (status) => setMouse({ enabled: status.settings.enabled, access: status.access }),
      () => setMouse(null),
    );
  }, []);

  useEffect(() => {
    if (!open) {
      return;
    }
    readMouse();
    // Back from System Settings: read every permission again (no polling).
    const onFocus = () => {
      refresh();
      readMouse();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [open, refresh, readMouse]);

  if (!state || !preferences || !pending) {
    return null;
  }

  const finish = () => {
    setOpen(false);
    // Not saved: it shows again at the next launch, and Kleari works meanwhile.
    void dismissIdentityMove().catch(() => undefined);
  };

  const notificationsWereOn =
    preferences.notifyScan ||
    preferences.notifyQuarantine ||
    preferences.notifyStorage ||
    preferences.notifyPerformance ||
    preferences.notifyBattery ||
    preferences.notifyLoginItems ||
    preferences.notifyReminders ||
    preferences.scanSchedule.frequency !== "off";

  const allowNotifications = () => {
    requestNotifications().then(
      (access) => {
        refresh();
        if (access !== "allowed") {
          setSteps("notifications");
        }
      },
      () => setNote("macOS couldn’t be asked about notifications. Turn them on in System Settings › Notifications."),
    );
  };

  const allowMouse = () => {
    requestMouseAccess().then(
      (status) => {
        setMouse({ enabled: status.settings.enabled, access: status.access });
        if (status.access !== "allowed") {
          setSteps("accessibility");
        }
      },
      () => setSteps("accessibility"),
    );
  };

  return (
    <Modal open={open} onClose={finish} labelledBy="identity-move-title">
      <div className="v2-dialog-body setup-body">
        <div className="v2-dialog-head">
          <IconTile tone="blue" icon="lock" size={44} glow />
          <div className="v2-row-text">
            <h2 id="identity-move-title" className="v2-dialog-title">Allow Kleari again</h2>
            <span className="v2-card-note">
              This version of Kleari has a new identity on your Mac, so macOS asks for its permissions again. Your history, settings and quarantine came across as they were.
            </span>
          </div>
        </div>

        {mouse?.enabled && mouse.access !== "unsupported" && (
          <section className="setup-step" aria-label="Accessibility">
            <IconTile tone="violet" icon="mouse" size={34} />
            <div className="v2-row-text">
              <strong className="v2-row-title">Accessibility</strong>
              <span className="v2-card-note">Your Mouse settings work again once Kleari is allowed. Also used by keyboard cleaning.</span>
              {mouse.access !== "allowed" &&
                (steps === "accessibility" ? (
                  <AccessSteps kind="accessibility" compact onOpen={() => void openSystemSettings("accessibility")} />
                ) : (
                  <Button variant="soft" size="sm" onClick={allowMouse}>Allow</Button>
                ))}
            </div>
            {mouse.access === "allowed" ? <Pill status="ok">On</Pill> : <Pill status="muted">Off</Pill>}
          </section>
        )}

        <section className="setup-step" aria-label="Full Disk Access">
          <IconTile tone="blue" icon="lock" size={34} />
          <div className="v2-row-text">
            <strong className="v2-row-title">Full Disk Access</strong>
            <span className="v2-card-note">Lets Kleari measure everything, including folders macOS protects. Skip it if you hadn’t turned it on before.</span>
            {!state.fullDiskAccess &&
              (steps === "fullDiskAccess" ? (
                <AccessSteps kind="fullDiskAccess" compact onOpen={() => void openSystemSettings("fullDiskAccess")} />
              ) : (
                <Button variant="soft" size="sm" onClick={() => setSteps("fullDiskAccess")}>Show me how</Button>
              ))}
          </div>
          {state.fullDiskAccess ? <Pill status="ok">On</Pill> : <Pill status="muted">Off</Pill>}
        </section>

        {notificationsWereOn && (
          <section className="setup-step" aria-label="Notifications">
            <IconTile tone="orange" icon="bell" size={34} />
            <div className="v2-row-text">
              <strong className="v2-row-title">Notifications</strong>
              <span className="v2-card-note">You had notifications on; macOS needs to allow them again.</span>
              {state.notifications !== "allowed" &&
                (steps === "notifications" ? (
                  <AccessSteps kind="notifications" compact onOpen={() => void openSystemSettings("notifications")} />
                ) : (
                  <Button variant="soft" size="sm" onClick={allowNotifications}>Allow</Button>
                ))}
            </div>
            {state.notifications === "allowed" ? <Pill status="ok">On</Pill> : <Pill status="muted">Off</Pill>}
          </section>
        )}

        <section className="setup-step" aria-label="Open at login">
          <IconTile tone="green" icon="power" size={34} />
          <div className="v2-row-text">
            <strong className="v2-row-title">Open at login</strong>
            <span className="v2-card-note">If Kleari opened when you logged in before, turn this on again.</span>
            {state.loginItem === "requiresApproval" && <AccessSteps kind="loginItems" compact onOpen={() => void openSystemSettings("loginItems")} />}
          </div>
          <Switch
            label="Open at login"
            checked={state.loginItem === "enabled" || state.loginItem === "requiresApproval"}
            disabled={state.loginItem === "unavailable" || state.loginItem === "unsupported"}
            onChange={(on) => {
              void settings.openAtLogin(on).then(setNote);
            }}
          />
        </section>

        <p className="v2-card-note">
          macOS may also ask again the first time Kleari reads your Downloads, Desktop or Documents folder, or a browser’s tabs.
          {pending.oldPermissionsReset ? "" : " System Settings may still list an older Kleari; you can remove it there with the − button."}
        </p>
        {note && <span role="status" className="v2-card-note">{note}</span>}
        <div className="v2-dialog-actions">
          <Button variant="primary" onClick={finish}>Done</Button>
        </div>
      </div>
    </Modal>
  );
}
