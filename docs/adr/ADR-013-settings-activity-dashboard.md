# ADR-013: Preferences, Activity, Dashboard and background System sampling

- **Status:** Accepted. Shipped in release builds by the product owner's decision. The
  menu-bar popover preview and "not built" list below are superseded by ADR-014 (menu-bar
  item, update checks, Uninstall).
- **Context:** Release builds showed Cleanup, Quarantine, Storage, System, Developer,
  Processes and the appearance setting. Dashboard, Activity and the rest of Settings were
  development-only sample screens. System read nothing until it opened, so it opened empty.
  The product owner decided:
  - System samples from launch.
  - Dashboard shows live readings and prompts, and never scans on open.
  - Settings makes Open at login, Cleanup defaults, notifications, and history and folders
    real.
  - Activity is live.
- **Decision:**
  - **System sampling.** An app-level monitor samples activity from launch:
    - every 5 s while System isn't on screen, every 1 s while it is;
    - only while Kleari is open to look at (the window visible, or the menu-bar popover open,
      which Rust reports with `popover-shown`) and not paused; the process list is sampled the
      same way (every 5 s, 2 s on Processes);
    - details and startup items are read at launch and again each time System opens.
    History is only what was sampled; a gap longer than three background intervals starts a
    new chart. The chart is labelled by the time its readings actually cover ("last 45 s"),
    not by its number of slots.
  - **Preferences** are stored in the history database (`setting` table, migration 3) as one
    validated record, and Rust applies them itself.
    - Every value must be one the Settings page offers; a stored value that isn't one is
      replaced by the defaults (shown as such), never used.
    - What they do:
      - **Default scan and scan options:** Cleanup starts with them.
      - **Minimum size:** findings smaller than it are left out of Cleanup's review. Off by
        default.
      - **Recent-file window:** large files changed within it aren't suggested, by Cleanup
        or Storage. Off by default.
      - **Quarantine reminder:** shown on the Dashboard and notified when crossed.
      - **Notifications:** which ones are sent, and with a sound or not.
      - **Activity retention:** 30–360 days in steps of 30, or a year.
    - Preferences only ever leave findings out; nothing becomes eligible to move because of
      them.
  - **Excluded folders** (`excluded_folder` table):
    - Cleanup scans never list, enter or measure them (`Context.excluded`, checked before
      any `stat`), and the Developer project search skips them and roots inside them.
    - A would-be finding that contains an excluded folder (a cache folder, a build folder,
      a Developer cache location) is left out whole and never measured: moving it would move
      the excluded folder too (`preferences::touches_excluded`).
    - Storage's large-file suggestions leave them out; its totals still measure them.
    - Removing one is named by its `~/…` form and matched against Rust's own list.
    - If the list can't be read (an unreadable history), nothing goes ahead as if it were
      empty (`History::exclusions`):
      - Cleanup and Developer scans, sending to review, preview, approval and Show in Finder
        for a review item refuse with `historyUnavailable`;
      - Storage keeps its totals but suggests no large files;
      - Quarantine verifies nothing, and leaves out an original place in Show in Finder and
        when settling.
  - **Applied again before anything moves.** Preview and approval check the current
    preferences and exclusions with every other check, in this order:
    1. whether the item is in or contains a folder excluded now (`clear_of_exclusions`),
       before anything is read, so an excluded folder is never measured again;
    2. then the size measured now and the modification time read now (`kept_now`; an
       unreadable time fails closed).
    An item left out since the scan isn't planned, and if it was, approval finds it changed
    and moves nothing. During approval both are checked again immediately before each item's
    rename, so an item changed while earlier ones moved stays in place (`changed`). Between
    that check and the rename a file could still be written in place with the same inode
    and size; that window is the same one ADR-012 accepts for the identity check.
  - **Developer project discovery** skips reading `.git/logs/HEAD` when it is in or holds an
    excluded folder, and doesn't look at a build folder in or holding one.
  - **Sending to review** (Developer's items, Storage's large files) drops items in or holding
    a folder excluded now before anything about them is read. The workspace lockfile search
    stops at an excluded folder and never looks at an excluded lockfile, manifest or `.git`;
    without what they would have shown, no lockfile is assumed (so `node_modules` needs a
    check rather than being eligible).
  - **Show in Finder** refuses a review item in or holding a folder excluded now, before
    reading its path (`cleanup::revealable`), and Quarantine's "Show both in Finder" leaves
    out an original place that is now excluded. Restoring into a folder excluded later is
    still allowed: it puts the user's own item back and reads nothing else there.
  - **Folder access** in Settings lists:
    - protected folders granted with Grant access; removing one forgets the remembered answer
      (macOS's own permission stays in System Settings);
    - folders added this session in Cleanup or Developer;
    - Quarantine's restore-only folders.
    Removing an added folder drops it from that session list.
  - **Open at login** uses `SMAppService.mainAppService` (macOS 13+), read back from macOS
    rather than remembered. macOS shows its own notice and keeps the switch in System Settings
    › General › Login Items. "Requires approval" offers that pane.
  - **Notifications** use the macOS notification centre (`UNUserNotificationCenter`).
    - macOS asks once, when a notification is first turned on; a refusal offers System
      Settings › Notifications.
    - They are sent only when the preference is on and macOS allows them at that moment
      (checked before each one), for a finished Cleanup scan, and when an approval takes
      quarantine past the reminder size (once, as it crosses).
    - Outside an app bundle (`tauri dev`) the notification centre isn't touched, since it
      would abort the process; notifications are then unavailable.
  - **Activity** is read from the history (`list_activity`), newest first, up to 500 events.
    - Recorded events: Cleanup scans, Storage analyses, Developer scans, items sent to
      review, quarantine moves, restores and permanent deletes (each with its items' outcomes,
      `history_event_item`), protected-folder answers, and quit requests (ADR-009).
    - An event stores counts, sizes, an outcome code and a subject: a scan mode, `developer` or
      `storage`, a protected folder's ID, or an app's name. Never a path.
    - Items are named with their original folder as `~/…`, joined from the quarantine records;
      outside the home folder only the folder's own name is shown (`…/DerivedData`).
    - `failed` counts every item an operation didn't complete, including those a stopped
      delete never reached (recorded as `kept`), so `items - failed` is what was done.
    - Events older than the retention preference are deleted at launch and when it changes.
      Clear log deletes them all. Quarantine records are never deleted with them (they are
      needed to restore what is held).
    - Export isn't offered: it needs its own privacy review.
  - **Dashboard** shows, without starting anything:
    - the monitor's readings (CPU with its sparkline, memory, battery) and the disk's
      capacity;
    - this session's last Storage analysis by category, or an Analyze button that starts the
      same read-only analysis as Storage;
    - the latest scan's largest eligible findings, or a Scan link;
    - what quarantine holds, and whether it is past the reminder size.
  - **One interface.** Release and development builds render the same shell with every screen
    live. Development builds add only the sample-data badge and the menu-bar popover preview,
    whose native item doesn't exist yet; `check:release-bundle` still proves no sample data
    ships.
  - **Capabilities:** ten commands are added to the default capability:
    - `get_settings`, `set_preferences`, `set_open_at_login`, `request_notifications`;
    - `add_excluded_folder`, `remove_excluded_folder`, `remove_folder_access`,
      `reveal_quarantine_folder`;
    - `list_activity`, `clear_activity`.
    The Notifications pane joins the fixed System Settings panes.
- **Consequences:**
  - The window samples activity in the background every 5 s while visible. It is a small
    cost, paused when the window is hidden or the user pauses System.
  - A record of what Kleari did now persists locally (ADR-006): scan counts and sizes,
    operations, folder answers and the names of apps asked to quit. It is deleted after the
    retention period or by Clear log.
  - Open at login and notifications need macOS to recognise Kleari by its signed bundle.
    A Tauri build only linker-signs the executable (identifier `kleari-<hash>`, bundle
    unsealed), which macOS's login items and notification centre don't accept, so the bundle
    is now signed ad hoc (`bundle.macOS.signingIdentity: "-"`, identifier
    `dev.bakare.kleari`, sealed resources, hardened runtime). A distributed build still
    needs a Developer ID signature and notarization. Without a usable signature they say so
    and stay off. On macOS 11 and 12, which lack `SMAppService`, Open at login is
    `unsupported`: it says it needs macOS 13 and points to Users & Groups › Login Items,
    rather than suggesting a move that can't help.
  - Quarantine's listing and launch reconciliation don't look at an original place in or
    holding a folder excluded now (`Scope::may_probe`): such an item is listed restorable
    without the conflict probe (the restore itself never overwrites and reports a
    conflict), and an interrupted restore isn't settled from that place.
  - Not built, so not shown in Settings: the menu-bar item, scheduled scans, the
    findings-limit notification, crash-report sharing, export and update checks.
