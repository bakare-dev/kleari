import { useEffect, useState } from "react";
import type { Settings } from "../../features/settings/useSettings";
import { DISCLAIMER_VERSION } from "../../features/legal/legal";
import { openLegalDocument } from "../../lib/legalIpc";
import { Button, IconTile, Modal } from "../ui/components";

/**
 * A friendly note (DISCLAIMER.md in short), shown once (and again whenever DISCLAIMER_VERSION
 * goes up) before anything else. It closes only with Got it, which records that it was seen.
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
            <h2 id="disclaimer-title" className="v2-dialog-title">A quick note before you start</h2>
            <span className="v2-card-note">Kleari is free and open source, made with care.</span>
          </div>
        </div>
        <p className="v2-dialog-text">
          Nothing happens to your files without your OK: Kleari shows you what it will do first, and anything it moves waits in quarantine so you can put it back. It’s
          provided as is, without a formal warranty.
        </p>
        <button type="button" className="v2-link" onClick={() => void openLegalDocument("disclaimer").catch(() => setNote("The disclaimer couldn’t be opened; it’s in DISCLAIMER.md on GitHub."))}>
          Read the full note
        </button>
        {note && <span role="status" className="v2-card-note">{note}</span>}
        <div className="v2-dialog-actions">
          <Button variant="primary" onClick={accept}>Got it</Button>
        </div>
      </div>
    </Modal>
  );
}
