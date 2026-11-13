# Kleari design v2: implementation handoff

This package is the approved v2 UI for Kleari, covering the glass-and-colour redesign, every screen in light and dark, and the Navigator logo. It was exported from the design canvas on 2026-10-18.

**Status:** this is approved design, not implemented behaviour. Where v2 differs from `docs/design/THEME.md` (glass surfaces, colourful icon tiles, the Navigator logo), v2 is the newer decision from the product owner. Update THEME.md, DESIGN.md and SCREENS.md as you implement each part, so the docs keep matching the app. The safety rules in CONTRIBUTING.md, the product requirements and the threat model still override anything here. No screen in this package grants a capability that those documents forbid.

## What's in the package

| Path | What it is |
|---|---|
| `screens/light/*.png`, `screens/dark/*.png` | Pixel reference for all 13 boards at 2x (1260×920 board, 1120×800 window). |
| `source/*.dc.html` | The interactive design source. The markup has exact sizes, spacing, radii and copy. The `<script>` block has the sample data, states and behaviour, and `theme()` holds the light and dark values. Read these when a PNG is ambiguous. |
| `tokens.css` | Every theme value as `--mp-*` CSS variables. Light is the default, and dark is declared twice (system dark and `data-theme="dark"`), matching the existing appearance setup. |
| `tokens.json` | The same values as JSON. |
| `brand/` | Navigator app icon (SVG and PNGs from 16 to 1024), the menu-bar template glyph, and a white glyph. |

The static PNGs don't show backdrop blur. The glass recipes below describe what the real surfaces should do.

## Global layout

- **Window:** 1120×800 design size, radius 14, and a 1px `--mp-window-edge` border. The page behind it is `--mp-wallpaper` and only exists for the mockups.
- **Sidebar:** 232px wide on `--mp-glass` with backdrop blur. In the real app, use the native Liquid Glass or vibrancy window material described in ADR-008.
  - Header: the Navigator icon (36px), the "Kleari" wordmark (filled with `--mp-wordmark`), and the tagline "Scan · Explain · Approve".
  - Nav order: Dashboard, Cleanup, Quarantine, Storage, System, Developer, Processes, Activity. Settings is pinned to the bottom.
  - Each nav item is 34px tall, radius 8, with a 10px gap between icon and label. The selected item uses `--mp-nav-sel` plus `--mp-nav-sel-shadow`, and its icon turns blue.
  - The footer chip reads "Offline. Nothing leaves this Mac." with a green dot.
- **Content:** `--mp-bg` background with 28px side padding. The page title is 28px, weight 700, letter-spacing -0.02em, with a 13px `--mp-text2` subtitle below it.
- **Cards:** `--mp-card`, a 1px `--mp-border`, radius 14 and `--mp-card-shadow`. In dark mode cards have no shadow.
- **Type:** the system font stack, 13px base and line-height 1.45. Paths use a monospace font.
- **Icon tiles:** 42px, radius 11, filled with a colour gradient, with `inset 0 1px 0 rgba(255,255,255,.45)` and a soft glow in the same hue.
- **Accent palette** (tiles, charts, process colours): red `#E03131`, blue `#2F6FED`, gray `#6B7686`, teal `#0891B2`, purple `#9333EA`, green `#1FA45B`, orange `#F5841F`, indigo `#5B4BE0`, pink `#E1306C`, yellow `#EAB308`.

## Glass recipes

1. **Scroll-edge glass.** At the top of every scrolling list, a sticky 40px band fades in once content scrolls under it.
   - The band is a top-to-bottom gradient from `--mp-edge-top` to `--mp-edge-mid` to transparent, with `backdrop-filter: blur(16px) saturate(1.9) brightness(1.04)`, masked to fade out toward the bottom (`mask-image: linear-gradient(180deg, #000 45%, transparent)`), a 1px `--mp-edge-line` highlight on top, and a `--mp-edge-sheen` radial sheen.
   - Fade-in in CSS: `animation: mp-glass-in linear both; animation-timeline: scroll(nearest block); animation-range: 0 28px;`, with an `onScroll` (scrollTop > 0) fallback.
   - Make the band `position: sticky; top: 0` with `margin-bottom: -(40px + gap)` so it doesn't push content down.
2. **Floating action bars** on Developer, Quarantine and Review.
   - The bar overlays the bottom of the list on a `--mp-float-top` to `--mp-float-bg` gradient, with `blur(24px) saturate(1.9)`, radius 18, a `--mp-float-edge` border and `--mp-float-shadow`.
   - Give the scroller enough bottom padding (about 104px) that the last row can scroll clear of the bar.
3. **Table header glass:** sticky headers use `--mp-th-glass` with blur.
4. **Popover:** the popover uses the glass treatment, floating under a glass menu bar.

## Screens

Each screen's behaviour lives in its `.dc.html` script. The summary below lists what to look for.

- **Dashboard (Main):** search field with ⌘K, the health summary, metric cards (CPU, Memory, Storage, Battery as gauge rings), findings ready for review, and recent activity.
- **Cleanup (step 1 of 3):** choose scan scope and categories, then run a read-only scan with progress and Stop. Results are worded "eligible for review", never "safe".
- **Review (step 2):** grouped findings with checkboxes, a risk label and a reason for each, and a floating glass bar showing the selection total that continues to Preview.
- **Preview and approve (step 3):** the exact list of what will move to quarantine. Moves only start after an explicit Approve. A warning banner covers anything unusual.
- **Quarantine:**
  - Stats (held size, item count, oldest item) and an info strip ("Kleari never empties quarantine on its own. Restores never overwrite…").
  - Batches grouped by cleanup. Each row shows a Restorable or "Blocked: file exists" status in a 150px column, plus a Restore button.
  - A floating bar with Delete permanently… and Restore.
  - Permanent delete opens a confirmation, then a **progress dialog**: a per-item progress bar, "Stop after this item", and a partial result if stopped.
- **Storage:**
  - A capacity bar segmented by category, where hover or select highlights a segment (inactive segments drop to 0.55 opacity in light, 0.75 in dark).
  - Categories, a Largest folders drill-down, and a Large files tab with "Add to review".
  - Docker data is labelled read-only.
  - "Analyze again" opens a **modal** with progress and Stop.
- **System:**
  - Four gauge cards: CPU, Memory, Battery, Thermals. Each has a label and status pill on top, then a ring with a value and sub-label ("CPU temp", "of 24 GB").
  - A Live/Pause toggle and an activity chart with CPU, Memory, Network and Disk tabs.
  - Mac specs, Health checks (Startup disk, Battery, macOS update with "Open Software Update"), and Startup and background items.
  - "Show all 8 items" opens a **modal**. There's no Time Machine or FileVault section.
- **Developer:** Toolchains and Projects tabs, with a detail card for the selection. A Docker card shows a copyable command, and Kleari never runs it. A floating glass bar offers "Select low risk" and "Send to review".
- **Processes:**
  - A fixed-height scrolling list with filters, sort and search.
  - Stats: Memory "of 24 GB", Running "processes", and Highest energy.
  - Selecting a process opens a **colourful side panel**, a hero gradient in that process's hue.
  - Quit and Force quit each need a confirmation. System processes are protected and can't be quit.
- **Activity:** the header, stats and filter tabs (All, Scans, Quarantine, …) stay fixed. Only the day-grouped list scrolls, with scroll-edge glass. Stats subs are "Plus 1 blocked" and "Each one confirmed".
- **Settings:**
  - Sections: General (including Appearance: System, Light, Dark), Scanning, Folders and access, Quarantine, Notifications, Privacy and safety, About.
  - Uses switches, segmented controls and steppers.
  - Offline mode and "never auto-empty quarantine" show a locked **Always on** badge.
  - Removing folder access and clearing the log each have a confirmation dialog.
- **Brand:** the Navigator mark, an orbit ellipse with a pilot arrow and a dot.
  - Sizes 112, 64, 32 and 16, the menu-bar glyph, and lockups on light, on dark and stacked.
  - Colour stops `#6CC4FF`, `#2F6FED`, `#6D3FE0`, plus usage rules.
- **Menu bar and popover:**
  - The menu-bar glyph has four states: Idle, Scanning (spins), Findings ready (dot), and Needs a look.
  - The glass popover has mini gauges and a scan card (ready, scanning with Stop, done), links, and Open Kleari and Quit buttons.

## Safety rules the UI must keep

- Scans are read-only. Results are "eligible for review", never "safe to delete".
- Nothing moves without the Preview and Approve step. Cleanup moves items to quarantine, never straight to deletion.
- Permanent delete always has its own confirmation, and shows progress with partial results.
- Quarantine is never emptied automatically. Restores never overwrite an existing file; those items show as Blocked.
- Docker items are read-only, and Kleari only shows commands for the user to run.
- System processes can't be quit. Quit and Force quit each need a confirmation.
- Offline mode is locked on, and the app says so.

## Dark mode

Every token has a dark value in `tokens.css`. Compare against `screens/dark/`. Points to watch:

- Cards drop their shadow and rely on the border.
- The wordmark uses the lighter gradient (`#7CC0FF` to `#B49CFF`).
- Status pills use translucent tinted backgrounds.

A few token values differ slightly between screens (`borderSolid`, `scrim`, `menuBg`, `menuShadow`, `warnBg`). `tokens.css` keeps the Dashboard's value. Pick one per token when you implement.

## Suggested build order

1. Add the tokens and the app shell (sidebar, window and Navigator icon).
2. Build shared components: card, icon tile, pill, gauge ring, segmented control, switch, modal, floating bar and scroll-edge glass.
3. Restyle the screens that already exist (Overview → Dashboard, Settings, Storage).
4. Add the rest one by one, each behind the backend capability it needs. Don't show a working control for something the backend can't do yet.
