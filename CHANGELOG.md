# Changelog

All notable user-facing changes to Kleari are documented here. This file follows
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/), and versions follow
[docs/versioning.md](docs/versioning.md).

## [Unreleased]

## [0.1.0-beta.5] - 2026-11-18

### Fixed

- **Dashboard:** no longer keeps re-reading its issues while it is open, which used processor
  time for nothing.
- **App leftovers:** a folder named only after an app (not its bundle ID) is no longer offered
  as its leftover while another installed or running app has the same name, since it may be
  that app's data.
- **Mouse window actions** (tile, close, minimise, full screen) now check that what the
  frontmost app reports is a window, button, position or size before using it, and skip the
  action otherwise.

## [0.1.0-beta.4] - 2026-11-18

### Added

- **Remove items where you see them.** Apps, Files, Storage, Developer and Maintenance's login
  items no longer send items to Cleanup's review: one confirm sheet lists what will go, with
  app icons and Show in Finder for each, and offers **Delete permanently** (deleted where it
  is, after a fresh check) or **Move to quarantine** (with Undo). Removing an app offers what
  it left behind straight after.
- **Trash in Kleari:** a Trash tab on the Quarantine page lists your Trash and each connected
  drive's, with where each item came from. **Put Back** returns an item to where Finder
  says it was (only to a plain place in your home folder or on the same disk, never over
  anything); **Delete permanently** and **Empty Trash** too. Needs Full Disk Access.
- **App caches:** a Clear caches button on each app in Apps, and an App caches tab in
  Maintenance listing apps by cache size. Never Apple's, never while the app is open, and
  never its settings or data.
- **More AI clutter:** Claude Code's saved sessions, shell snapshots, task lists, feature-flag
  cache and debug logs (never its settings, CLAUDE.md or memory), Codex sessions and logs,
  Gemini CLI temporary files, and what AI coding tools made directly in `/tmp`.
- **Pictures:** thumbnails and a large preview (with the rest of a group in a strip, arrow
  keys to move) in Duplicates, Similar Images, Large & Old, Downloads, Storage Browse and the
  Trash; real app icons in Apps, Browser care and Maintenance.
- **One filter bar on every list** (Cleanup Review, Trash, Files, Apps, Developer, App
  caches): largest, smallest, newest, oldest or name order, type chips with counts and a name
  search. Each list remembers its order and type; the search lasts until Kleari quits.
- **Files › Downloads:** open a folder in the results and keep going deeper, with pictures and
  removal as in Storage.
- **Storage:** Reveal in Finder for a large file from the last analysis, and the action bar
  in Browse stays in view once something is chosen.
- **Dashboard search** looks through everything Kleari already holds this session (pages,
  Settings, the last scans' results, quarantine, processes and the activity log), grouped by
  page; a result opens its page and tab with that list's search filled in. It never starts a
  scan; a page not scanned yet says so. Clearing the search brings the Dashboard back.
- **Clean the keyboard** (Maintenance › Tasks, or the menu-bar panel for a minute): locks the
  keyboard, media and brightness keys and macOS's shortcuts for 30 seconds to 5 minutes so you
  can wipe it. Any mouse or trackpad click unlocks it, and so do the time running out and
  quitting Kleari. Needs Accessibility; nothing about the keys is read or kept.
- **Mouse:** new actions (Show Desktop, Spotlight, next/previous app and window, tabs, Refresh,
  Zoom, scroll, screenshots and recording, minimise, full screen, close, tile left/right, hide
  app, Lock Mac); shortcuts are recorded with macOS's own shortcuts such as ⌘⇥ captured, with
  Use and Cancel; the Mouse page lists the external mice connected.
- **A local log and Report a problem.** Kleari keeps `~/Library/Logs/Kleari/kleari.log` on
  your Mac (what it did and what went wrong, never keys, file names, paths, window titles or
  addresses; at most about 2 MB). Settings › About › Help › **Report a problem** opens a new
  GitHub issue filled in with Kleari's version, the macOS version and the kind of chip, and
  shows the log in Finder so you can choose to attach it. Nothing is sent unless you submit it.
- **Install steps in the disk image window:** drag to Applications, then how to open Kleari
  the first time.
- **Legal documents and a "use at your own risk" notice.** Kleari is free, open-source software
  provided as is, and only acts on what you approve: [DISCLAIMER.md](DISCLAIMER.md) says so
  plainly, and a short notice is shown once at first launch (and to existing users once, after
  updating). Settings › About has a Legal section with the
  disclaimer, [PRIVACY.md](PRIVACY.md), the licence, [SECURITY.md](SECURITY.md) and the new
  [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), generated from every package that ships, with
  a CI check that it stays current and that each licence can be combined with Kleari's. The ⓘ at
  the bottom of the sidebar shows the notice.

### Security

- Kleari's window now blocks every outside connection by name (`connect-src`), objects,
  frames, forms and base URLs, on top of loading nothing from outside the app. A new CI
  check (`pnpm check:network`) fails any change that would let Kleari reach the internet
  other than for updates, and [docs/network.md](docs/network.md) lists every address it
  contacts and how to check it yourself.

### Changed

- **Kleari's identifier on your Mac is now `dev.kleari.app`** (was `dev.bakare.kleari`). At the
  first launch after updating, Kleari moves your history, settings and quarantine to the new
  name on its own, and macOS forgets the old name's permissions. A one-time window lists the
  permissions you used (Accessibility for Mouse, Full Disk Access, notifications, Open at
  login) with a button for each, so you can allow them again.
- **Uninstall** asks macOS to forget Kleari's permissions itself (`tccutil reset All
  dev.kleari.app`) before moving anything; it gives you the command only if that failed. It
  also moves Kleari's log to the Trash.
- **One Kleari at a time:** opening Kleari while it runs brings the running one forward
  instead of starting a second copy (two copies handled every mouse event twice).
- **Apps** finds apps in folders inside Applications (two levels down, never inside an app)
  and lists macOS's own apps as part of macOS, never removed. Apple apps from the App Store,
  such as Xcode and Logic Pro, can now be removed; their Library data is still never offered.
  A rescan keeps the page usable and keeps what you chose.
- **Leftovers in use are never offered:** anything a running process has open, works in or
  runs from is skipped, and a launch agent is offered only when its program is gone.
- **The menu-bar panel** sizes itself to what it shows, up to the room above the Dock.
- **Mouse** leaves trackpad clicks alone (it's for external mice), and a system shortcut is
  turned off while recording only if it still has its usual key.
- Buttons say what they do in full ("Remove 3 items", "Quit Safari"), and dialogs fit inside
  the window and scroll when taller.
- Downloads' untagged items are "Saved or copied here", with why, instead of "Unknown".
- Similar Images decodes photos at a small size: much faster and lighter on large and RAW
  photos.
- Space freed by removing duplicates now reads "up to", since copies sharing storage on APFS
  may free less.
- Maintenance, Storage and Trash pages are laid out again; the window opens wider.
- **Clearer download and install notes.** The README now says before the download link that
  Kleari isn't notarized by Apple and that macOS asks once, with no Gatekeeper changes or
  Terminal commands needed. The install guide no longer asks you to sign in to GitHub (the
  repository is public), and the README's privacy line says exactly what goes online: update
  checks and downloads, nothing else.

### Fixed

- **Settings › About › What's new** showed the release page's address as text you couldn't
  click; it now opens the release notes in your browser, as does the update offer.
- **Files:** a type filter showed everything in a group that had none of that type.
- **Apps:** an app removed during a scan could come back from that scan's results.

### Removed

- **Mouse:** the Do nothing, Middle click and media-key actions. A button set to one goes back
  to its usual click and a combination set to one is removed; everything else you set stays.
  "Last app used" becomes Next Application.
- **Maintenance:** the "Not offered" tab.

## [0.1.0-beta.3] - 2026-11-14

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

[Unreleased]: https://github.com/bakare-dev/kleari/compare/v0.1.0-beta.5...HEAD
[0.1.0-beta.5]: https://github.com/bakare-dev/kleari/compare/v0.1.0-beta.4...v0.1.0-beta.5
[0.1.0-beta.4]: https://github.com/bakare-dev/kleari/compare/v0.1.0-beta.3...v0.1.0-beta.4
[0.1.0-beta.3]: https://github.com/bakare-dev/kleari/compare/v0.1.0-beta.2...v0.1.0-beta.3
[0.1.0-beta.2]: https://github.com/bakare-dev/kleari/compare/v0.1.0-beta.1...v0.1.0-beta.2
[0.1.0-beta.1]: https://github.com/bakare-dev/kleari/releases/tag/v0.1.0-beta.1
