import type { UpdateState } from "../../../features/settings/useUpdates";
import type { UpdateCheck } from "../../../contracts/updates";
import { Button, IconTile, Modal } from "../../ui/components";

/** One line on where updates stand. */
export function updateStatus(state: UpdateState): string {
  switch (state.phase) {
    case "idle":
      return "Not checked since Kleari opened";
    case "checking":
      return "Checking GitHub…";
    case "checked":
      switch (state.check.status) {
        case "available":
          return `Kleari ${state.check.version} is available. You have ${state.check.current}.`;
        case "noRelease":
          return state.check.beta
            ? `No beta is out yet. You have ${state.check.current}.`
            : `No stable release is out yet. You have ${state.check.current}; turn on Beta updates to get betas.`;
        case "upToDate":
          return `You have the latest version, ${state.check.current}.`;
      }
      break;
    case "installing":
      return state.check.status === "available" ? `Downloading and installing ${state.check.version}. Kleari restarts when it’s done.` : "Installing…";
    case "failed":
      return state.message;
  }
}

/** The update to offer an Install button for, if any. */
export function installable(state: UpdateState): UpdateCheck | null {
  const check = state.phase === "checked" || state.phase === "failed" ? state.check : undefined;
  return check?.status === "available" ? check : null;
}

/**
 * Offered once at launch when the automatic check finds an update. Nothing installs until
 * you choose Install and restart.
 */
export function UpdateOffer({ offer, state, onInstall, onLater }: { offer: UpdateCheck | null; state: UpdateState; onInstall: () => void; onLater: () => void }) {
  const installing = state.phase === "installing";
  const open = offer?.status === "available";
  return (
    <Modal open={open} onClose={() => !installing && onLater()} labelledBy="update-title" className="set-dialog">
      {offer?.status === "available" && (
        <div className="v2-dialog-body v2-dialog-centered">
          <IconTile tone="blue" icon="info" size={52} glow />
          <h2 id="update-title" className="v2-dialog-title">Kleari {offer.version} is available</h2>
          <p className="v2-dialog-text">You have {offer.current}. Installing replaces the app and restarts it; your settings, history and quarantine stay as they are.</p>
          {offer.notes && <p className="v2-dialog-text set-update-notes">{offer.notes}</p>}
          {state.phase === "failed" && <p role="alert" className="v2-dialog-text">{state.message}</p>}
          <div className="v2-dialog-actions">
            <Button variant="secondary" disabled={installing} onClick={onLater}>Later</Button>
            <Button variant="primary" disabled={installing} onClick={onInstall}>{installing ? "Installing…" : "Install and restart"}</Button>
          </div>
        </div>
      )}
    </Modal>
  );
}
