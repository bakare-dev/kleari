# Changelog

All notable user-facing changes to Kleari are documented here. This file follows
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/), and versions follow
[docs/versioning.md](docs/versioning.md).

## [Unreleased]

### Added

- **Cleanup › Preview:** **Delete permanently instead…** next to Quarantine. After its own
  confirmation, the items are checked and moved into quarantine as usual, then deleted from
  there straight away; anything that can't be deleted stays in Quarantine.
- **README:** Buy me a malt, as another way to support Kleari.
- **Alerts:** critical disk space (under 5%), critical memory pressure, a processor busy for
  15 minutes, battery low (20%) and critical (10%), fully charged, hot, near its cycle limit
  and "service recommended", many login items, a new login item, and a weekly cleanup
  reminder when no scan ran for 7 days.
- **Settings › Notifications › Alerts:** one switch per group (Storage, Performance, Battery,
  Login items, Reminders). If you had alerts on, every group stays on.
- **System:** the processor's temperature from the chip's own sensors (average and hottest),
  graphics where the chip reports it, and the fans' speed with their range, in a new Cooling
  check. Read only: Kleari never changes fan speeds.
- **Menu bar:** the CPU tile shows the processor's temperature and the fan speed.
- **Menu bar:** the alerts that need attention, network download and upload rates, and when
  your Mac last started up, how long it has been on and when it last woke.

### Changed

- **Lighter on CPU:** the process list is read only while Processes or Browser care is on
  screen, and live readings only while the Dashboard or System is; the live dot no longer
  repaints the page every frame. The frosted band under the header of scrolling pages, and
  the blur under the Review table's headings, are gone.
- **After a cleanup** moves or deletes items, the pages they came from (Developer, Apps,
  Files, Storage) scan again, so none still lists what is gone.
- **Preview and approve:** the page stays still and only the list of items scrolls; long
  names, sizes and what happens to each no longer overlap.
- **Maintenance and Files:** the tabs and the tool stay in place; only the items scroll.
- **Storage › Browse:** the colour map of the folder is larger and no longer shrinks to a
  sliver beside a long folder.
- **Dashboard:** the four readings wrap two by two in a narrow window.
- **System › Temperature** (was Thermals): the processor's temperature, or the battery
  sensor's where the chip's can't be read, with macOS's thermal state, each named for what it
  is.
- **Menu bar:** Activity is no longer listed; the menu is a little taller to fit the alerts.
- **Low disk space** is now flagged below 15% free (was 10%).
- **Small windows:** summary cards drop their icon and wrap their label instead of cutting it
  off; Activity's search moves under its filters; System's section headings wrap.

### Fixed

- **Browser care** listed 0 tabs in 0 windows for every browser; tabs are listed again, and
  faster with many tabs.
- **CPU** showed "Measuring…" for a long time on the Dashboard, and System's readings dropped
  out while Kleari's window wasn't active.
- **Sizes** under 1 MB showed as "0 MB"; they show in KB, and Cleanup's last scan no longer
  rounds small categories to 0 GB.
- **Battery health** showed a higher maximum capacity than System Information (for example
  99% where macOS said 96%); it now shows macOS's own figure.
- **Preview and approve:** a long item name no longer disappears behind its category, and
  after a cleanup the result list gets the room the approval steps took.
- **Developer:** a project at your home folder (a stray `npm install` in `~`) was always
  "Active project" because something in your home folder is always open; for it, only its
  build folders being open counts.

## [0.1.0-beta.2] - 2026-11-13

### Added

- **Apps:** every app you installed, with what it keeps in your Library. **Remove** moves an
  app to the Trash (Put Back works) after you confirm, and sends what it left in your Library
  to Cleanup's review. Also lists data left by apps that are gone and installers in Downloads
  and on the Desktop, to send to review. Nothing in your Library moves until you approve it,
  and then only into quarantine.
- **First-launch setup:** Kleari offers Full Disk Access, Open at login and Show in the menu
  bar when it first opens; each is optional.
- **Step-by-step guides** wherever Kleari asks for a permission (Full Disk Access,
  Accessibility, App Management, Automation, Files and Folders, notifications, login items),
  saying exactly what to click in System Settings.
- **Mouse:** switch apps with a held button and the wheel (like ⌘⇥: let go to choose), step
  through an app's windows (⌘`), and buttons for "last app used" and "next window".
- **Browser care:** CPU and memory of Safari, Chrome, Brave and Edge, their tabs (after you
  allow it in macOS's Automation prompt), and closing the tabs you choose.
- **Issue alerts** on the Dashboard: a nearly full disk or drive, memory pressure, fast
  battery drain, a Bluetooth device's low battery, a large Trash and apps not opened in six
  months, each with where to deal with it; notified while Kleari runs if you turn it on in
  Settings › Notifications.
- **Maintenance:** free up memory, flush the DNS cache, free up purgeable space, reindex
  Spotlight, clear system caches and old system logs, remove deleted users and empty the
  Trash, each when you choose it, with macOS's own password prompt where needed; and login and
  background items, where you can stop one of your own. What isn't offered says why.
- **Files:** Large & Old files, exact Duplicates (one copy always kept), Similar Images,
  Downloads by the app that downloaded them, and AI clutter (model downloads, AI apps' caches
  and logs). What you choose goes to Cleanup's review and moves only into quarantine.
- **Cleanup › Broken items:** damaged preferences files and login items whose program no
  longer exists, found by Standard and Deep scans. **Xcode previews** are found by the
  Developer scan.
- **Storage › Browse:** open any folder in your home folder down to single files, with a map of
  what takes the most space, and send what you choose to Cleanup's review. It moves only when
  you approve it, into quarantine; never while something in it is open in an app.

### Changed

- **Active projects** are now projects with a file, or the folder itself, open in an app
  (an editor, a terminal or a dev server), checked when you scan and again before anything
  moves. A recent Git commit no longer makes a project active, so a project you aren't
  working on can be selected.
- **Hidden files off** now skips only private dotfiles such as `.env` and `.ssh`. Build and
  cache folders such as `.next`, `.turbo` and `.venv` are always searched for large and old
  files.
- **Large and old files** say what kind of file they are (video, disk image, archive, …)
  instead of "Large file Kleari can't classify".
- **Activity** reads the log when Kleari opens, so the page isn't blank while it loads.
- **Large files added to review from Storage** can now be moved to quarantine when they are in
  your home folder (they were inspect only).

### Fixed

- The sidebar and the menu-bar popover showed an old name instead of "Kleari".
- The menu-bar popover appeared without its styling in release builds.

## [0.1.0-beta.1] - 2026-11-13

The first release of Kleari.

### Added

- **Dashboard:** storage, memory, CPU and battery at a glance, the latest scan's largest
  findings and what quarantine holds.
- **Cleanup:** Standard, Deep and Developer scans that only read; each finding explained with
  its risk; nothing moves until you approve a preview, and then only into quarantine. Items
  an app is using, Apple's caches and recently committed projects' build folders are never
  offered.
- **Scheduled scans** (Settings › Scanning): daily or weekly, read-only, with a notification
  that opens the review.
- **Quarantine:** restore anything (never overwriting what is there now), or delete
  permanently after a separate confirmation.
- **Storage:** your startup disk by folder and category, space macOS manages, and files of
  1 GB or more.
- **System:** live CPU, memory, swap, battery, thermal state, network and disk activity, this
  Mac's details and its startup items. Readings keep going while Kleari's window or its
  menu-bar panel is open, so the page opens with recent charts.
- **Processes:** what is running, apps grouped with their helpers, with a confirmed, polite
  Quit for regular apps. Like System, the list is kept up to date while Kleari is open.
- **Developer:** developer caches, project build folders and Docker's disk use (read-only).
- **Mouse:** reverse wheel scrolling for a mouse only, speed and smooth scrolling, button
  actions and recorded shortcuts, media keys, and combinations of up to two buttons (left or
  right click included) with ⌃⌥⇧⌘, while scrolling or when pressed. A drawn mouse shows
  which button is which.
- **Menu bar** panel with live readings, the last scan, quarantine and activity.
- **Activity** log, kept for as long as you choose.
- **Updates** from Kleari's releases, signed and installed only when you choose, and
  **Beta updates** in Settings › About to also get betas.
- **Uninstall** from Settings › About, moving Kleari and its data to the Trash.

[Unreleased]: https://github.com/bakare-dev/kleari/compare/v0.1.0-beta.2...HEAD
[0.1.0-beta.2]: https://github.com/bakare-dev/kleari/compare/v0.1.0-beta.1...v0.1.0-beta.2
[0.1.0-beta.1]: https://github.com/bakare-dev/kleari/releases/tag/v0.1.0-beta.1
