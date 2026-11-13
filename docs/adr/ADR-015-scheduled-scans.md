# ADR-015: Scheduled scans

- **Status:** Accepted, by the product owner's decision. This is the
  background lifecycle and permission design ADR-004 asks for, for this one task.
- **Context:** Cleanup scans ran only when the user started one. The product owner decided
  that Kleari should scan on a schedule, notify what it found, and open the review when the
  notification is clicked; nothing moves without the user's approval, as before.
- **Decision:**
  - **Preference** `scanSchedule` in `preferences.v1` (serde default, so earlier records
    load with it off): frequency `off` (default), `daily` or `weekly`; a weekday (0 is
    Monday) for weekly; a local hour, 0–23; and the scan mode. The scan uses the saved Cleanup
    scan options. Settings › Scanning › Schedule edits it.
  - **Lifecycle.** A thread started at launch (`schedule::start`) checks once a minute, so
    scans run only while Kleari runs: open, or in the menu bar (ADR-014). Open at login keeps
    it running after a restart. No launch agent, daemon or wake timer is added.
  - **When it scans** (`schedule::decide`, pure and tested). The latest scheduled time is
    computed from the local wall clock (`localtime_r`), so on a day the clocks change it can be
    an hour out. A record (`scan_schedule.v1`: the schedule it refers to, the time up to which
    scheduled times are dealt with, and the last scheduled scan) decides:
    - a schedule just turned on or changed starts counting from now, without scanning at once;
      turning it off is recorded as a change too, so turning it back on later doesn't catch up
      the times it was off;
    - a time that passed since the last one dealt with scans once, so times missed while
      Kleari wasn't running or the Mac slept are caught up once, not once each;
    - the record is written before the scan, so a failing scan isn't retried every minute.
  - **Never in the way.** A check waits (and tries again a minute later) while a scan or a
    quarantine move, restore or delete runs. If the settings can't be read, nothing runs.
  - **The scan** is the same code as a Cleanup scan (`cleanup_commands::run_scan`): read-only,
    the same exclusions and preferences, only folders already granted or added. It never asks
    for folder access, so no macOS prompt appears unannounced. Its findings replace the review
    as a scan the user starts does, and Activity records it as a scheduled scan.
  - **Notification.** A scheduled scan always notifies what it found, if macOS allows
    Kleari's notifications (turning the schedule on asks macOS first if it hasn't been
    asked; refused, the findings wait in the review and Settings says so). A scan the user
    starts still notifies only if turned on. Clicking either notification opens the main window
    at the review (`UNUserNotificationCenter` delegate, `notifications::open_review_on_click`).
    Kleari's notifications now also show while Kleari is in front.
  - **Open windows** read the review and activity again when a scheduled scan finishes (the
    `scheduled-scan-finished` event).
- **Consequences:**
  - Kleari reads the disk on its own, at the times the user chose, while it runs. It is the
    same read-only scan; nothing is moved, deleted or sent.
  - A scheduled scan replaces the review, including one the user was working through (as a
    second manual scan would); items sent from Storage or Developer stay.
  - A scan never starts while the Mac is asleep: a time missed then runs once at the next check
    after waking.
