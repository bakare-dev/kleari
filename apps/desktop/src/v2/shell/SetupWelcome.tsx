import { useEffect, useState } from "react";
import type { Settings } from "../../features/settings/useSettings";
import { openSystemSettings } from "../../lib/systemPageIpc";
import { AccessSteps } from "../ui/AccessSteps";
import { Button, IconTile, Modal, Pill, Switch } from "../ui/components";
import { NavigatorIcon } from "./brand";

/**
 * The first-launch setup: Full Disk Access (with the steps to turn it on), Open at login and
 * Show in the menu bar. Shown once; every choice can be changed later in Settings.
 */
export function SetupWelcome({ settings }: { settings: Settings }) {
  const { state } = settings;
  const preferences = state?.preferences.preferences ?? null;
  const [open, setOpen] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [showSteps, setShowSteps] = useState(false);
  const { refresh } = settings;

  useEffect(() => {
    if (preferences && !preferences.setupDone) {
      setOpen(true);
    }
  }, [preferences]);

  // Back from System Settings: read Full Disk Access again.
  useEffect(() => {
    if (!open) {
      return;
    }
    const onFocus = () => refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [open, refresh]);

  if (!state || !preferences) {
    return null;
  }

  const finish = () => {
    setOpen(false);
    void settings.save({ ...preferences, setupDone: true });
  };

  return (
    <Modal open={open} onClose={finish} labelledBy="setup-title">
      <div className="v2-dialog-body setup-body">
        <div className="v2-dialog-head">
          <NavigatorIcon size={48} />
          <div className="v2-row-text">
            <h2 id="setup-title" className="v2-dialog-title">Welcome to Kleari</h2>
            <span className="v2-card-note">Three things that make Kleari work best. Each is optional and can be changed later in Settings.</span>
          </div>
        </div>

        <section className="setup-step" aria-label="Full Disk Access">
          <IconTile tone="blue" icon="lock" size={34} />
          <div className="v2-row-text">
            <strong className="v2-row-title">Full Disk Access</strong>
            <span className="v2-card-note">Lets Kleari measure everything, including folders macOS protects. Kleari still only reads them, and nothing leaves your Mac.</span>
            {state.fullDiskAccess ? null : showSteps ? (
              <AccessSteps kind="fullDiskAccess" compact onOpen={() => void openSystemSettings("fullDiskAccess")} />
            ) : (
              <Button variant="soft" size="sm" onClick={() => setShowSteps(true)}>Show me how</Button>
            )}
          </div>
          {state.fullDiskAccess ? <Pill status="ok">On</Pill> : <Pill status="muted">Off</Pill>}
        </section>

        <section className="setup-step" aria-label="Open at login">
          <IconTile tone="green" icon="power" size={34} />
          <div className="v2-row-text">
            <strong className="v2-row-title">Open at login</strong>
            <span className="v2-card-note">Starts Kleari when you log in, so scheduled scans and alerts can run.</span>
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

        <section className="setup-step" aria-label="Show in the menu bar">
          <IconTile tone="violet" icon="dashboard" size={34} />
          <div className="v2-row-text">
            <strong className="v2-row-title">Show in the menu bar</strong>
            <span className="v2-card-note">Live readings and quick actions from the menu bar, and Kleari keeps running when its window is closed.</span>
          </div>
          <Switch
            label="Show in the menu bar"
            checked={preferences.menuBar}
            onChange={(on) => {
              void settings.save({ ...preferences, menuBar: on }).then(setNote);
            }}
          />
        </section>

        {note && <span role="status" className="v2-card-note">{note}</span>}
        <div className="v2-dialog-actions">
          <Button variant="primary" onClick={finish}>Done</Button>
        </div>
      </div>
    </Modal>
  );
}
