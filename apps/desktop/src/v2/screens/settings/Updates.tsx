import type { UpdateState } from "../../../features/settings/useUpdates";
import type { UpdateCheck } from "../../../contracts/updates";
import { openReleasePage } from "../../../lib/updatesIpc";
import { Button, IconTile, Modal } from "../../ui/components";

/** The release page's address as the update feed writes it into the notes. */
const RELEASE_LINK = /\s*(?:See\s+)?https:\/\/github\.com\/bakare-dev\/kleari\/releases\/tag\/\S+/g;

/** The notes without the release page's address, which is offered as a link instead; `null`
 * when nothing else is left. */
export function notesText(notes: string | null): string | null {
  const text = (notes ?? "").replace(RELEASE_LINK, "").trim();
  return text === "" ? null : text;
}

/** Opens the release notes on GitHub in the browser (a plain link would open inside Kleari). */
export function showReleaseNotes(version: string): void {
  openReleasePage(version).catch(() => {
    // Deliberate: if macOS couldn't open the browser there is nothing more Kleari can do, and
    // the update itself is unaffected.
  });
}

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
          {notesText(offer.notes) && <p className="v2-dialog-text set-update-notes">{notesText(offer.notes)}</p>}
          <button type="button" className="v2-link" onClick={() => showReleaseNotes(offer.version)}>
            See what’s new in {offer.version}
          </button>
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
