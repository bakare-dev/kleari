import { useEffect, useState } from "react";
import type { Settings } from "../../features/settings/useSettings";
import { DISCLAIMER_VERSION } from "../../features/legal/legal";
import { openLegalDocument } from "../../lib/legalIpc";
import { quitKleari } from "../../lib/menubarIpc";
import { Button, IconTile, Modal } from "../ui/components";

/**
 * "Use at your own risk", shown once (and again whenever DISCLAIMER_VERSION goes up) before
 * anything else. It can't be dismissed without a choice: I understand, or Quit Kleari.
 */
export function DisclaimerNotice({ settings }: { settings: Settings }) {
  const preferences = settings.state?.preferences.preferences ?? null;
  const [open, setOpen] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    if (preferences && preferences.disclaimerSeen < DISCLAIMER_VERSION) {
      setOpen(true);
    }
  }, [preferences]);

  if (!preferences) {
    return null;
  }

  const accept = () => {
    void settings.save({ ...preferences, disclaimerSeen: DISCLAIMER_VERSION }).then((problem) => {
      if (problem) {
        // Not saved: it asks again next time, and Kleari works meanwhile.
        setNote(problem);
      }
      setOpen(false);
    });
  };

  return (
    <Modal open={open} onClose={() => undefined} labelledBy="disclaimer-title" role="alertdialog">
      <div className="v2-dialog-body setup-body">
        <div className="v2-dialog-head">
          <IconTile tone="orange" icon="warning" size={44} glow />
          <div className="v2-row-text">
            <h2 id="disclaimer-title" className="v2-dialog-title">Before you start</h2>
            <span className="v2-card-note">Kleari is a personal project, made for fun and shared freely.</span>
          </div>
        </div>
        <p className="v2-dialog-text">
          It comes <strong>as is, with no warranty</strong>, and you use it at your own risk. Kleari can move and delete files, remove apps, run maintenance tasks with
          administrator rights and change how your mouse works. It asks first, and what it moves can be restored from quarantine until you delete it, but a mistake or a
          bug could still cost you data.
        </p>
        <p className="v2-dialog-text">Keep a current backup (Time Machine, for example), and read each preview before you approve it.</p>
        <button type="button" className="v2-link" onClick={() => void openLegalDocument("disclaimer").catch(() => setNote("The disclaimer couldn’t be opened; it’s in DISCLAIMER.md on GitHub."))}>
          Read the full disclaimer
        </button>
        {note && <span role="status" className="v2-card-note">{note}</span>}
        <div className="v2-dialog-actions">
          <Button variant="secondary" onClick={() => void quitKleari()}>Quit Kleari</Button>
          <Button variant="primary" onClick={accept}>I understand</Button>
        </div>
      </div>
    </Modal>
  );
}
