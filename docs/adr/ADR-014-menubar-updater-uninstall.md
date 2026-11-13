# ADR-014: Menu-bar popover, in-app updates and in-app uninstall

- **Status:** Accepted, by the product owner's decision, with beta and stable update
  channels.
- **Context:** The app had no menu-bar item, no way to update other than downloading again,
  and uninstalling meant finding its folders by hand. The product owner decided:
  - a menu-bar popover with live readings, quarantine and quick links; closing the window
    keeps Kleari running in the menu bar, with a Settings switch;
  - updates that check GitHub, ask, install and restart, signed with a free updater key (no
    Apple Developer ID, by the owner's standing decision);
  - Uninstall in Settings › About that needs quarantine dealt with first, turns off Open at
    login, moves the app and its data to the Trash, quits, and gives the step for macOS
    permissions.
- **Decision:**
  - **Menu-bar item** (`menubar.rs`, Tauri's `tray-icon` feature, no new crate). A 44×44
    template image from `icons/tray.rgba` (raw RGBA, so no image decoder). A left click opens
    or closes the popover: a borderless, always-on-top, 340×580 window labelled `popover`,
    with the system material behind it (Liquid Glass on macOS 26 and later, the popover
    material before, as in ADR-008).
    - It is placed under the icon in points, on the screen the icon is on (each screen is
      tried in its own scale), kept 8 points inside that screen's edges.
    - It is created hidden with the item, so the first click opens it at once. It closes on
      losing focus, on Escape, and when a link is chosen. A click on the icon within 300 ms of
      a focus-loss close doesn't reopen it.
    - **Show in menu bar** (default on, preference `menuBar`). While the item is on screen,
      closing the main window hides it; Quit (popover, app menu, ⌘Q) ends Kleari and the
      Dock icon brings the window back. Whether the item is on is read from the item itself
      (`menubar::active`), never remembered, so it can't claim an icon that failed to appear.
      With no item, closing the main window quits (`app.exit(0)`), and turning the item off
      destroys the popover window, so Kleari never runs with nothing on screen.
    - Changing the switch shows or removes the item first, on the main thread, and saves only
      if that worked (`menuBarUnavailable` otherwise). If the save then fails, the item is put
      back as it was. Saves are serialised in Rust. At launch a failure to show the item is
      ignored: Kleari opens, and closing the window quits as usual.
  - **Popover** (`PopoverApp`, chosen in `main.tsx` by window label, in every build). It
    shows the Dashboard's readings (storage, memory, CPU, battery), the last Cleanup scan,
    what quarantine holds and the latest activity.
    - It counts as open only while its window has focus. Nothing is read before it first
      opens; activity is sampled every second while it is open and stops when it closes; the
      rest is read once per opening. An earlier value kept through a failed read is marked
      "not updated".
    - It only reads. Links open the matching screen in the main window (`open_main` takes a
      fixed destination enum, so the route set in the page can't be injected); nothing in it
      starts a scan.
    - Its capability (`capabilities/popover.json`) allows only those reads plus `open_main`,
      `hide_popover` and `quit_kleari`.
  - **Updates** (`updates.rs`, `tauri-plugin-updater`, used from Rust only).
    - **Two channels**, like Apple's software update. The manifest is one of two fixed in
      `updates.rs`, chosen by the preference **Beta updates** (Settings › About, `betaUpdates`).
      Until the user chooses, it is unset (`null`) and follows the installed version, as
      macOS's betas do: a beta build gets betas, a stable build only stable releases
      (`Preferences::gets_betas`). Once chosen, the choice stays across versions.
      - **stable:** `github.com/bakare-dev/kleari/releases/latest/download/latest.json`.
        Stable releases are regular GitHub releases marked Latest; betas are pre-releases,
        which `latest` never returns, so the stable channel never sees a beta.
      - **beta:** `github.com/bakare-dev/kleari/releases/download/beta-channel/latest.json`,
        on a rolling pre-release whose manifest every release (beta or stable) replaces, so it
        always names the newest of both ([release-checklist](../release-checklist.md)).
      The updater never offers a lower version, so turning betas off keeps the installed beta
      until a newer stable release exists. `tauri.conf.json` keeps the stable manifest as the
      plugin's default; `check_for_update` always sets the endpoint explicitly. A channel
      with no manifest yet (no stable release before the first one, the plugin's
      `ReleaseNotFound`) is reported as `noRelease`, not as a failure; network and signature
      failures still fail.
    - `check_for_update` fetches that file and reports up to date, or the new version and its
      notes (cut to 1,000 characters). It runs once at launch when **Check for updates
      automatically** is on (default on, preference `checkForUpdates`), and from Settings ›
      About › Check now. The launch check runs only when that choice was actually read (saved,
      or no record yet): if the saved settings can't be read or used, it doesn't run, since the
      choice may have been off. A launch check that fails is shown only in About; one that finds an
      update offers it in a dialog (Later or Install and restart).
    - Both commands refuse (`updateNeedsApp`) unless Kleari runs from an installed `.app`:
      the updater replaces the running bundle, and a development build's would be Cargo's
      `target/debug` folder. In development the launch check therefore fails quietly (shown
      only in About).
    - An update is offered and downloaded only if its link is an HTTPS asset of Kleari's own
      releases (`https://github.com/bakare-dev/kleari/releases/download/…`, no other host
      or port); anything else fails the check. The plugin also refuses non-HTTPS downloads.
    - `install_update` downloads exactly the update the last check found, then takes the
      quarantine operation lock and holds it until the process restarts (so no move, restore
      or delete starts or is cut off), verifies the minisign signature against the public key
      in `tauri.conf.json`, and replaces the app. With `requireSignedVersion` the signature
      must also name the announced version, so a feed can't pair a higher version number with
      an older signed build. Any failure leaves the installed app as it was (`updateFailed`).
    - The private key and its password are kept outside the repository and CI. Updater artifacts are built only for releases, from `tauri.release.conf.json`
      (`createUpdaterArtifacts`), so CI and everyday builds need no key.
  - **Uninstall** (`uninstall.rs`, `uninstall_kleari`), in Settings › About.
    - Refused (`uninstallNeedsApp`) unless Kleari is running from an installed `.app`: a
      development build shares the installed app's data and has no app to remove. This is
      checked first, before anything else.
    - Under the quarantine operation lock, refused (`quarantineNotEmpty`) while quarantine
      holds anything: a record not restored or deleted, or anything in its folder besides
      `.DS_Store`. If either can't be read, it counts as holding something.
    - Then tries to turn Open at login off; if macOS keeps it on, the result says so with its
      reason and where to remove it.
    - Then moves to the Trash, with `NSFileManager trashItemAtURL`, only Kleari's own
      locations: in `~/Library`, Application Support, Caches, WebKit and HTTPStorages
      (`dev.bakare.kleari`), Saved Application State (`dev.bakare.kleari.savedState`) and
      Preferences (`dev.bakare.kleari.plist`); and last the running app's own bundle. Only
      locations that don't exist are skipped; one that can't be checked or moved is reported.
    - The result lists what was moved and what wasn't, gives `tccutil reset All
      dev.bakare.kleari` for macOS's permissions (Kleari can't clear those itself), and
      offers only Quit.
- **Consequences:**
  - Kleari now makes one kind of internet request: the update check, to GitHub, at launch
    (unless turned off) and when asked. It sends nothing about the Mac or the user's files
    beyond what any download sends (IP address, a user agent). The "Offline" wording in the
    interface became "Your files never leave this Mac".
  - An in-app update keeps Kleari's permissions only while every release is signed with the
    same certificate ("Kleari Self-Signed"); the updater key and the code-signing
    certificate are separate, and both must be kept.
  - WebKit may write its storage once more as Kleari quits after an uninstall, recreating
    `~/Library/WebKit/dev.bakare.kleari` with the appearance choice only. The install guide
    says so.
  - The popover window, while it exists, keeps a second WebView in memory (made at launch
    when the menu-bar item is on).
