# ADR-020: Maintenance tasks with macOS's own administrator prompt

- **Status:** Accepted, by the product owner's decision (asked for an administrator prompt per
  action rather than user-level only). It amends CONTRIBUTING's safety rule, which named only
  Cleanup's quarantine, Quarantine's restore and permanent delete, and Apps' move to the Trash.
- **Context:** People asked for maintenance other Mac utilities offer: free up memory, flush
  DNS, free up purgeable space, reindex Spotlight, clear system caches and logs, remove deleted
  users, empty the Trash, and manage login and background items. Most need administrator
  rights. CONTRIBUTING forbids a root helper and shell commands built from data.
- **Decision:**
  - **No helper.** Each task is a constant AppleScript (`maintenance.rs`) run by
    `/usr/bin/osascript` only when you choose it and confirm. Those that need rights say
    `with administrator privileges`, so macOS shows its own password prompt every time; Kleari
    never sees the password and keeps no rights. Every shell command in them is an absolute
    path with fixed arguments; nothing is built from data.
  - **Tasks:** `purge`; `dscacheutil -flushcache` and `killall -HUP mDNSResponder`;
    `tmutil thinlocalsnapshots / 999999999999 4`; `mdutil -E /`; deleting other apps' entries
    directly in `/Library/Caches` (never `com.apple.*`, `find -x`, depth 1); deleting rotated
    `.gz`/`.bz2` logs in `/private/var/log` and crash reports in
    `/Library/Logs/DiagnosticReports` older than 30 days; deleting `/Users/Deleted Users`; and
    Finder's own Empty Trash (macOS asks once to let Kleari control Finder).
  - **Deleting for good.** The four tasks that delete say so and need a separate
    acknowledgement, like Quarantine's permanent delete. System files can't go to quarantine:
    they belong to root and to macOS.
  - **Measured first, read-only:** the Trash (home and connected drives), `/Library/Caches`
    without Apple's, the old logs, deleted users, and the number of local snapshots
    (`tmutil listlocalsnapshots /`, no rights).
  - **Login and background items** are the list the System page already reads. One of your own
    launch agents can be sent to Cleanup's review (`Kind::LaunchAgent`, Medium risk), so it
    moves into quarantine only when approved and can be restored. Items for every user and
    system daemons are managed in System Settings, which the page opens.
  - **Not offered, with the reason shown:** repairing disk permissions and running periodic
    scripts (obsolete), removing language files and universal binaries (breaks app
    signatures), document versions (protected), Mail's Trash (Mail's own data).
  - Each run is recorded in Activity with its outcome (done, cancelled, failed).
- **Consequences:** Kleari can now delete system caches, old logs, deleted users' images and
  the Trash for good, each only after a confirmation and macOS's prompt. The threat model notes
  that these scripts run as root through macOS's prompt.
