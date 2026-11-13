# Screen specifications

These specifications establish hierarchy and expected states; they do not authorize
implementing the underlying feature. Product requirements and safety policy remain the
authority for capability.

## Design v2 implementation status

Every v2 board in [`v2/`](v2/HANDOFF.md) is built in `apps/desktop/src/v2/`.

- **Release builds** ship every screen on real data: **Dashboard**, **Cleanup** (Scan,
  Review, Preview and approve) and **Quarantine** (ADR-012), **Storage**, **System**,
  **Developer**, **Processes** (ADR-009), **Activity** and **Settings** (ADR-013). They open
  on the Dashboard.
- **Development builds** (`pnpm dev`, `pnpm tauri dev`) render the same interface with a
  sidebar badge, "Development build · live data". (Release builds show no badge.) The
  menu-bar popover (ADR-014) is live in every build.

`pnpm check:release-bundle` (run in CI) fails if the release interface's source imports reach
any file in `src/v2/sample/`, or if the sample-data marker appears in the release bundle.

| Screen | Route | Notes for wiring to the backend |
|---|---|---|
| Dashboard | `#dashboard` | **Live** (ADR-013). Changes from the design, agreed with the product owner: readings come from the System monitor sampled since launch, with dashes until the first one; the storage card shows this session's last Storage analysis by category, otherwise Analyze (the same read-only analysis as Storage; opening the page never scans); Recent findings are the latest scan's largest eligible items, otherwise a Scan link; Quarantine in Quick actions says how much is held when it passes the reminder size. "Scan again" is "Open Storage". Search hands its query to Review. |
| Cleanup · Scan | `#cleanup` | **Live** (ADR-012). Changes from the design, agreed with the product owner: Add folder opens the macOS folder panel instead of the in-app picker sheet; Grant access works for each protected folder (Downloads, Desktop, Documents, Movies), and a refusal offers Files and Folders in System Settings; with Full Disk Access they show "✓ Full Disk Access"; a banner offers Full Disk Access when macOS kept items private; while scanning, the active step has a spinner and a live count, with found-so-far totals, elapsed time and a moving bar; results are by category; the last-scan card appears once something was found; Storage's Scan in Cleanup opens this page with the mode chosen and never starts it; unlike other screens, it has no frosted band under the header when scrolled (owner's decision). |
| Cleanup · Review | `#review` | **Live.** Selection is shared with Preview; only eligible findings can be selected, and the backend decides which. Extra categories beyond the design: Build folders, Logs and temp, Old files. Large and old files, and Storage's sent files, are always inspect only. Category chips show only categories that have items. |
| Cleanup · Preview and approve | `#preview` | **Live.** Arriving makes the preview (each item checked and measured again); items left out are listed with a banner; approval needs the acknowledgement; an out-of-date preview, or an unwritable history, blocks it; a partial result names why the item stayed. The result says "Saved in Activity", where the move is listed. |
| Quarantine | `#quarantine` | **Live.** Restore conflicts are blocked, never overwritten ("Blocked: file exists", "Blocked: folder gone", "Not in quarantine folder"). Permanent delete has its own confirmation and per-item progress with "Stop after this item", and failures stay held. The page itself doesn't scroll: only the list of batches scrolls, under the scroll-edge glass as on Processes. A damaged history shows a banner instead of an empty list. |
| Storage | `#storage` | **Live** (see architecture.md, Storage page data). Changes from the design, agreed with the product owner: Analyze measures fixed locations, and anything unreadable makes sizes minimums ("at least", "≥"); used space not counted is a "Not measured" category; Available is Finder's figure, with purgeable space as an estimate; a hint opens Full Disk Access when it is off; large files have Add to review (listed in Cleanup to inspect, never moved) and the developer and caches categories have Scan in Cleanup, while the quarantine and review chips are not shown; the local snapshots row and the "Sorting into categories" step are dropped; large files are locked (Docker data, Xcode managed, macOS) or show no action. Nothing is analyzed until the user asks; the result is kept for the session only. The page itself doesn't scroll: the "What is taking space" card fills the remaining height and only its folder or file rows scroll, under the scroll-edge glass as on Processes, starting at the top again when a folder is opened; the category and "Space macOS manages" cards scroll only when the window is too short for both. |
| System | `#system` | **Live** (see architecture.md, System page data). Changes from the design, agreed with the product owner: Thermals shows the macOS thermal state, not °C (no public temperature API); Health checks are Battery and macOS (no SMART, and no "update available", which would need the network); Startup items are launch agents and daemons whose status says only what was checked ("Program found", "Program missing", "Not checked", "Plist: disabled"), without the design's Impact and Allowed columns, which macOS doesn't expose without administrator access. Readings are sampled from launch (every 5 s off the page, every second on it, ADR-013), so the page opens with them; the chart is labelled by the time its readings cover ("Activity · last 45 s"). The browser preview says readings need the desktop app. |
| Developer | `#developer` | **Live** (see architecture.md, Developer page data, and ADR-011). Changes from the design, agreed with the product owner: nothing is shown until you choose Scan; projects come from the home folder plus folders you add with Add folder; versions only where a version file exists (otherwise the location); low and check-first rows can be selected and sent to Cleanup's review ("Select low risk" adds only low), Xcode-managed and active rows can't, and sent rows show "In review"; a project row sends each of its build folders; Docker shows its breakdown only while it is running, otherwise its disk image's size. Docker is read-only: Kleari shows a command to copy and never runs it. The page itself doesn't scroll: the list card fills the remaining height and only its rows scroll, under the scroll-edge glass as on Processes; the detail and Docker cards scroll only when the window is too short for both. |
| Processes | `#processes` | **Live** (see architecture.md, Processes page data, and ADR-009; quit requests are recorded in Activity). Changes from the design, agreed with the product owner: no Energy column or "Highest energy" card (macOS has no public energy impact), so the card is "Busiest"; no "Not responding" state (no public API); no app-specific subtitles (an app's subtitle is its process count; other processes show where they run from); Quit is the polite request for regular apps only, never apps that ship with macOS (Finder, Terminal), with no Force quit, and the toast says the app was asked, not that it quit or freed memory. Other accounts' processes are listed using Apple's `ps` (ADR-010); their threads show "Not available". The CPU chart covers only the time the page is open. The list holds its order while the pointer or focus is in it. |
| Mouse | `#mouse` | **Live** (ADR-016). Not in the original design; added by the product owner's decision. A master switch in the header; a banner explaining what Kleari sees and asking for Accessibility (with Open System Settings) until it is allowed; a "Find your buttons" card with a drawn mouse whose parts light up as they are pressed or scrolled in Kleari's window (works before Accessibility is allowed; says whether the wheel scrolls in steps or smoothly; clicking a part goes to its setting; while the pointer is over the box it is highlighted with a "Testing" badge, and buttons 3–8, the wheel and right click only light up the drawing, with the mouse settings paused there); then Settings-style groups: Scrolling (reverse, sideways reverse, speed, smooth), Buttons (buttons 3–8, each an action or a recorded keyboard shortcut), and Combinations (up to 8: each up to two buttons, left or right click allowed with another button, plus ⌃⌥⇧⌘ toggles, While scrolling or When pressed, with its action; an incomplete one says why it isn't saved yet). A status card above the finder says whether the settings are on now and what would stop them (Show in menu bar or Open at login off, buttons never found). A banner if macOS didn't start it. |
| Activity | `#activity` | **Live** (ADR-013). Only the day-grouped log scrolls. Changes from the design, agreed with the product owner: events are what the history recorded (scans, analyses, Developer scans, sent to review, moves, restores, deletes, folder answers, quit requests), worded from their outcome, with item lines as name and `~/…` folder; no separate "Approved" event (the move records the approval); no Export (needs its own privacy review); older entries are deleted after the retention period, not summarised; an unreadable history shows a banner, never an empty log. |
| Settings | `#settings` | **Live** (ADR-013). Changes from the design, agreed with the product owner: Scanning has Schedule (Off/Daily/Weekly, day, hour and scan mode, with the next and last scheduled scan; ADR-015). Not shown because they aren't built: the findings-limit notification, Share crash reports, Export, Help and Acknowledgements. General has Show in menu bar; About has Updates (automatic check, Check now, Install and restart) and Uninstall (ADR-014). Default scan offers Standard, Deep and Developer. Minimum size and the recent-file window can be Off (the default); the recent-file window applies to large files. Allowed folders lists granted protected folders and this session's added and restore-only folders; Allow another folder opens Cleanup. Open at login and notifications read their state from macOS and offer System Settings when it needs approval or refused. Activity is kept 30–360 days or a year. A failed save shows the reason and the last saved value. |
| Menu-bar popover | Kleari's menu-bar icon | **Live** (ADR-014). Its own 340×580 window under the icon: the four readings, the last scan (Open Cleanup, Review), Quarantine and Activity rows, Open Kleari and Quit. It reads only while open and starts nothing. |

Actions that would reach outside Kleari in a real build (Finder, System Settings, export)
show "Sample data: Kleari would …" and change nothing. When a flow is wired, its sample
controller in `src/v2/sample/` is replaced by the backend; the screen components take typed
props and don't change. The Brand board is a reference, not a screen; its mark is the app icon.

## Main window frame

The app shell provides persistent navigation, a page title/context and the active screen.
Show local/offline status only when it reflects actual app state. Keep global controls out of
content tables. At the current 680×520 minimum, allow sidebar collapse and contained table
scrolling rather than clipping.

## Dashboard

- **First view:** concise system overview with timestamped CPU, memory, disk and battery
  readings where available; show scan/operation state separately.
- **Hierarchy:** page title and freshness → key metrics → recent operation summary → links
  to storage and developer areas. Do not promote cleanup as the only primary action.
- **Actions:** navigate to a detail view or begin a separately described read-only scan.
- **Empty:** explain that readings or history are not yet available and why; no fabricated
  metrics. **Error:** show which source failed, its last known freshness and retry path.

## Cleanup

- **First view:** scan scope, last scan time, scan status and explanation that scanning is
  read-only. Before first scan, ask the user to choose a location and grant access.
- **Hierarchy:** scope/permission → scan control → findings summary grouped by category and
  risk/eligibility → review action.
- **Actions:** start/cancel scan; open results; review a specific eligible preview. No
  cleanup runs without the separate preview and explicit approval flow.
- **Empty:** distinguish “not scanned,” “scan found no candidates,” and “partial/no access.”
  **Error:** preserve completed results, identify skipped/inaccessible scope and offer safe
  retry. Never imply broader permission was requested automatically.

## Scan results and review

- **First view:** scope, scan time/freshness, count and partial-result status; then a stable,
  sortable, filterable list with item, size, category, reason, uncertainty, risk and policy
  eligibility represented separately.
- **Actions:** inspect details, select eligible rows for a preview, clear selection, rescan.
  Selection itself has no side effect. Eligibility is backend supplied.
- **Empty:** explain no findings versus filtered-out findings. **Error:** identify stale,
  incomplete or inaccessible data; stale items cannot be approved.

## File or cleanup-item details

- **First view:** display name and scoped path representation, size, kind, observed time,
  classification evidence, uncertainty and freshness. Avoid unnecessary path disclosure in
  global surfaces.
- **Actions:** inspect parent/context, include in a review if policy allows, or exclude.
  Clearly state if no operation is available. **Empty:** item removed from current results.
  **Error:** item disappeared/changed or access was revoked; invalidate related preview.

## Storage explorer

- **First view:** selected scope and total/known/unknown coverage; then a solid-surface
  hierarchical breakdown and sortable file inventory with readable units.
- **Actions:** change scope, search, filter and open item details. No implicit cleanup.
- **Empty:** request a scope or explain unreadable/unscanned areas. **Error:** show partial
  coverage, last refresh and retry without representing unknown space as zero.

## System monitoring

- **First view:** current readings with source, units and freshness; historical trends only
  when actually collected and available.
- **Actions:** choose supported time range, pause/resume foreground refresh if supported.
  No background monitoring claim unless the feature and permission are active.
- **Empty:** metric unavailable or unsupported on this OS/device. **Error:** retain last
  reading with age and mark stale; explain recovery.

## Developer tools

- **First view:** grouped inventory of supported developer caches/artifacts, source scope,
  scan freshness and classification explanation.
- **Actions:** inspect, filter and review eligible findings. Tool-specific paths are never
  assumed safe merely because they are recognizable.
- **Empty:** no supported locations or no findings, with cause. **Error:** access denied,
  unknown version or partial inventory is explicit and does not broaden access.

## Process overview

- **First view:** process name, resource reading, source and sample freshness; sort controls
  are clearly named and stable.
- **Actions:** inspect process details and refresh. No force quit or process mutation is
  implied by an overview screen.
- **Empty:** explain that no sample is available. **Error:** identify unsupported/denied
  source and retain an honest stale/unavailable state.

## Quarantine and restore

- **First view:** quarantined items, original location in appropriately private detail,
  quarantine date, status and recovery limitations.
- **Actions:** restore after checking conflicts; remove permanently only through a distinct
  destructive flow. Restoration must not overwrite an existing destination.
- **Empty:** no quarantined items and no retention claim beyond implemented policy.
  **Error:** report partial quarantine/restore failure and recovery state; quarantine failure
  never falls through to permanent deletion.

## Activity and action history

- **First view:** chronological operations with outcome, time, scope summary and partial or
  recovery status. Redact sensitive path detail by default.
- **Actions:** open operation detail; retry only where an explicit safe retry contract
  exists; export only after an explicit privacy review.
- **Empty:** no operations recorded. **Error:** explain unavailable/corrupt history without
  showing an operation as successful when state is unknown.

## Settings

- **First view:** appearance, privacy/permissions, scan preferences and history controls,
  grouped under clear headings. Show current permission state.
- **Actions:** change user-facing preferences, open macOS permission settings when
  supported, inspect data handling. No silent permission expansion or retention deletion.
- **Empty:** a preference may be unset with a stated default. **Error:** show failed save
  and keep the last confirmed value.

## Menu-bar popover

- **First view:** concise current status and freshness, then a direct path to the main app.
  Fit a compact transient panel; translucency is optional and contrast-tested.
- **Actions:** open main app and supported read-only quick views. Never place permanent
  deletion or ambiguous cleanup behind a compact popover.
- **Empty:** clear unavailable/never sampled state. **Error:** one-line reason and route to
  details; no false live status.

## Confirmation dialogs and important overlays

- **Scan permission:** name exact scope, read-only purpose and OS grant; denial remains a
  valid choice.
- **Operation preview/approval:** enumerate exact selected items, effects, uncertainty,
  risk, eligibility, quarantine destination/recovery limits and preview freshness. Approval
  binds only to that preview; changed identity, scope, policy or permission invalidates it.
- **Permanent deletion:** distinct from quarantine and restore; explain irreversibility and
  exact affected items; require deliberate, clearly labeled confirmation.
- **Restore conflict:** explain that destination exists and will not be overwritten; offer
  cancel and safe resolution choices only when implemented.
- **Errors/progress:** explain what completed, what failed, cancellation status and next
  safe step. Do not dismiss unresolved destructive-operation results automatically.
