# ADR-009: Asking an app to quit from the Processes page

- **Status:** Accepted. Shipped in release builds by the product owner's decision (see
  Consequences).
- **Context:** The Processes page shows what is running and the design offers to quit apps.
  Every other Kleari feature only reads. Quitting changes the state of the Mac, so it
  needs a narrow, reviewed boundary, as the product requirements ask of any operation:
  explicit confirmation and backend revalidation immediately before acting.
- **Decision:**
  - **What can be quit:** only a regular macOS app (`NSApplicationActivationPolicy::Regular`)
    of the signed-in account, from outside macOS's own folders.
    - Never Kleari itself, an app that ships with macOS (`/System`, `/usr`, `/sbin`,
      `/Library/Apple`, which covers Finder and Terminal), a background process or a system
      process.
    - The backend decides this per row (`canQuit`, from `may_quit`) and gives the row an
      identity token: the app process's start time to the microsecond. The contract
      requires the token exactly when `canQuit`, and the command checks everything again.
  - **How:** only the polite request (`NSRunningApplication terminate`), the same as
    choosing Quit from the app's menu. The app may ask to save, or decline.
  - **Excluded:** force quit, signals to background processes, and anything for other
    accounts or system processes. Each would need its own decision.
  - **Confirmation:** a confirmation dialog in the page precedes every request.
  - **Binding to the row:**
    - A row gets a token only if the PID's identity, read after the sample, is still the
      sampled process (same start second), and its bundle resolves.
    - Resolving follows symlinks one component at a time through directory handles
      (`storage::fsdir`: each folder opened relative to its open parent with no-follow and an
      identity check, `fstatat(NOFOLLOW)`, `readlinkat`). A parent swapped mid-walk can't
      redirect it, and `/Applications/Safari.app` resolves into `/System/Cryptexes` and stays
      protected.
    - Resolving doesn't touch anything at or below a mount that the mount table, read just
      before (`getmntinfo(MNT_NOWAIT)`, no file-system I/O), reports as non-local: network
      shares and automounts. Such apps, or any app when the mount table can't be read, are not
      quittable, so a refresh doesn't contact or stall on another computer through a known
      mount.
    - The backend keeps each token with the row's PID and resolved bundle. Each sample
      replaces them.
  - **Revalidation:** `quit_app(pid, quitToken)` first looks the token up in the backend's
    record from the latest list (unknown token, or another PID: `notRunning`). It then runs
    every check and the request in one main-thread step:
    1. Refuse Kleari's own PID.
    2. Read the PID's identity (`proc_pidinfo` BSD info: start time in microseconds and
       owner UID). It must equal the token and belong to this account; otherwise the PID is
       gone or now another process's (`notRunning`), or not ours (`notAllowed`).
    3. Look the app up by PID (`NSRunningApplication`). It must be a regular app that hasn't
       terminated, and its resolved bundle must equal the recorded one and lie outside
       macOS's folders.
    4. Read the identity again, immediately before `terminate`.
  - The frontend sends only the PID and the token, never a path.
  - **Outcomes** are reported, never assumed: `requested`, `notRunning`, `notAllowed`,
    `declined`. The page says "Asked X to quit", never that it quit or freed memory.
  - **Permissions:** `list_processes` and `quit_app` are granted by the default capability.
    The page was first development-only, behind a debug-only capability. The product owner
    then chose to release it with the Developer page.
- **Consequences:**
  - **Recorded (ADR-013).** Each request macOS delivered (`requested`) or didn't
    (`refused`) is recorded in the history with the app's name (its bundle's name, never its
    path) and shown on the Activity page. Requests refused before reaching macOS (not
    allowed, no longer running) aren't recorded. Until ADR-013 nothing was recorded, an
    accepted gap the product owner released Processes with.
  - **Residual mount race:** a network volume mounted over a component of an app's path
    after the mount table is read, and before the walk reaches that component, could still be
    looked up once. The startup-item check has the same limitation. Detecting it would need
    the very lookup it guards against. The mount table is re-read for every refresh and for
    every quit, so the window is the few milliseconds of one walk.
  - **Residual race:** the remaining window is between the second identity read and
    `terminate`, microseconds on the same thread. A reused PID would also need a new process
    started in that window that is itself a regular, quittable app of this account, and the
    request is still the polite one.
  - **Release:** shipped together with the Developer page. Native verification of Quit by the
    maintainer is still to be done before that release is published.
