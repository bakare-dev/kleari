# ADR-026: One Kleari at a time

- **Status:** Accepted, by the product owner's decision.
- **Context:** With a development build and a release running together (seen 2026-11-17),
  both installed the Mouse module's event tap, and every wheel and button event was handled
  twice. macOS lets only one copy of an app run per bundle, but two builds are two bundles to
  it, so its own rule doesn't cover this. A second launch should bring the first one forward
  instead.
- **Decision:**
  - **A lock in the data folder** (`instance.rs`). Every build shares the data folder
    (`~/Library/Application Support/dev.kleari.app`). At launch, after the identifier move
    (ADR-024) and before Tauri starts, Kleari opens `kleari.lock` there (0600, not following a
    symbolic link) and takes an exclusive, non-blocking `flock` on it, held as long as the
    process runs. macOS releases it when the process ends, crash included, so there is no
    stale lock to clear.
  - **A later launch quits at once** when the lock is held, before any window, tap or menu-bar
    item exists, after posting a distributed notification named `dev.kleari.app.show`,
    delivered immediately (a background app otherwise has distributed notifications held
    back). It carries nothing else.
  - **The running Kleari** observes that name from launch and shows its main window (as a
    Dock click does) when it arrives. Nothing polls; the observer only runs when posted to.
  - If the lock can't be made or read, Kleari runs anyway, since one copy is the usual case.
- **Consequences:** No new dependency (feature flags of objc2-foundation only) and no socket.
  A development build won't start while the release runs, and the other way round: quit one
  first. Any local process could post the notification; all it can do is show Kleari's
  window.
