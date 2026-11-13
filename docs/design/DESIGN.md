# Kleari visual design contract

**Status:** supporting visual contract. This document describes intended UI; it does not
claim that planned product features exist. Theme values and identity are owned by
[THEME.md](THEME.md) and are Kleari decisions, not Apple's specifications.

## Product philosophy

Kleari should feel like a considered macOS utility for people who care about their
machines and their files. Be calm, precise, privacy-conscious and technically credible.
Let useful information lead; use polish to clarify hierarchy rather than decorate it.

- Scan freely. Explain clearly. Execute only with explicit authorisation.
- Local-first and useful without an account, cloud, or AI.
- Show evidence, freshness, uncertainty, eligibility and effects separately. A visual risk
  label is never permission to act.
- Prefer native-feeling conventions, predictable navigation and direct language.
- Do not suggest that a scan, operation, permission or system reading happened unless it
  actually did.

## Visual direction

Use the restrained macOS-inspired identity and palette in [THEME.md](THEME.md). The brand
should feel capable and approachable, not clinical, playful or alarmist. Use the system
font stack and do not copy Apple marks or imply Apple endorsement.

Apple's design references inform the use of layered navigation materials and adaptation.
Kleari is a web-rendered Tauri frontend. CSS opacity and `backdrop-filter` can suggest
translucency; they do not reproduce Apple's native Liquid Glass material, lensing, adaptive
contrast or system-integrated behavior. Genuine native material requires native macOS
integration and must be described accurately. The main window's sidebar now uses that
integration (Tauri window effects: Liquid Glass on macOS 26+, sidebar vibrancy on macOS
14–15) with an overlay title bar; see [ADR-008](../adr/ADR-008-native-window-material.md).

## Theme and semantic tokens

[THEME.md](THEME.md) is authoritative for product identity, light/dark colors, typography,
spacing, geometry and shared token roles. This document does not define a competing palette.
The CSS implements THEME.md's palette; derived tokens are recorded there. Use tabular numerals for aligned measurements.

## Surface and material rules

- **Solid:** default for tables, inventories, scan results, storage breakdowns, activity,
  settings, warnings and forms. Stable opaque surfaces protect legibility.
- **Translucent:** reserve for the menu-bar popover, contextual menus, tooltips and small
  transient overlays when underlying content benefits contextually. Add a sufficiently
  opaque tint and border; never rely on blur for text contrast.
- **Floating:** dialogs, popovers and transient controls may use elevation and a restrained
  shadow. Keep one clear floating layer; avoid glass-on-glass and nested blur.
- Provide a solid fallback under `prefers-reduced-transparency: reduce`, when
  `backdrop-filter` is unavailable, and whenever contrast cannot be guaranteed.
- Do not put moving or dense content behind critical controls. Glass is an accent, not the
  default content material.

### Design v2 glass recipes (implemented in `src/v2/v2.css`)

The approved v2 design ([`v2/HANDOFF.md`](v2/HANDOFF.md)) adds four glass treatments. Each
has an opaque fallback under Reduce transparency.

1. **Scroll-edge glass** (`ScrollEdgeGlass`, `ScrollBody`): a sticky 40 px band at the top of
   every scrolling list that fades in once content scrolls beneath it. It uses a scroll-driven
   CSS animation where supported and a scroll-position fallback otherwise. A negative bottom
   margin keeps it from pushing content down.
2. **Floating action bars** (`FloatingBar`) on Review, Quarantine and Developer: glass over
   the bottom of the list. The scroller has about 104 px of bottom padding so the last row
   scrolls clear of the bar.
3. **Table header glass** (`.v2-th-glass`): sticky table headings.
4. **Popover and side panels**: the menu-bar popover, the Review details drawer and the
   Processes panel.

Content cards, tables and forms stay on opaque `--mp-card`. Colourful gradient icon tiles
(`IconTile`, accent palette `.tone-*`) are decorative and always paired with text; status is
still carried by labelled pills, never by colour alone.

### Design v2 shared components (`src/v2/ui/components.tsx`)

`IconTile`, `Pill`, `Dot`, `Button`/`LinkButton` (primary, secondary, soft, danger,
dangerOutline, plain), `Segmented` (a radio group with arrow-key navigation and a roving
tab index for settings; a labelled group of pressed buttons for view and filter switchers,
so nothing is announced as tabs without tab panels), `Switch`, `Stepper`,
`GaugeRing`, `ProgressBar` (with an in-progress sheen), `Spinner`, `Modal`, `Toast`,
`FloatingBar`, `ScrollBody`/`ScrollEdgeGlass`, `SearchField`, `StatCard`, `PageHeader` and
`LiveBadge`.

- `Modal` is built on the native `<dialog>`, so focus is trapped, the page behind is inert
  and Escape closes it. Confirmations (`alertdialog`) don't close on a backdrop click. A
  running permanent delete can't be dismissed; it is stopped explicitly.
- `Toast` messages live in the top layer, so they are never hidden behind an open dialog.

## Component standards

- **Buttons:** label the action with a verb; one clear primary action per decision area.
  Destructive actions use a danger treatment and explicit wording, not merely a red icon.
- **Cards:** group related metrics or explanatory content only. Do not wrap every control
  and table in a large rounded card.
- **Navigation:** persistent sidebar in the main window, with selected state, icon and text.
  Keep section names stable and avoid navigation hidden only on hover.
- **Tables:** solid background, persistent column labels, aligned numeric values, visible
  selected row and keyboard-operable row actions. Selection never executes an action.
- **Badges:** compact text plus color/icon where useful. Distinguish risk, policy eligibility,
  operation state and data freshness as separate concepts.
- **Search and filters:** visible labels or accessible names, clearable values, explicit
  active-filter count, and a useful no-results state.
- **Progress:** show a determinate amount only when known. Indeterminate progress must say
  what is happening; include cancel when safe and supported.
- **Dialogs:** state the target, effect, risk, recovery limits and required decision. Put
  cancel first in keyboard order when the action is consequential; do not preselect a
  destructive action.
- **Notifications:** concise outcome, no sensitive full path by default, actionable detail
  in-app, and no alarmist tone.

## Interaction states

Every interactive component defines default, hover, focus-visible, pressed, selected,
disabled, loading and error states as applicable. Persistent selections must not resemble
hover. Disabled controls explain why when the reason is material. Success and warning
states use a label/icon and accessible announcement. Do not communicate state by hue alone.

## Accessibility

- Target WCAG 2.2 AA contrast: 4.5:1 for normal text, 3:1 for large text and meaningful
  non-text control boundaries/indicators. Confirm with rendered values.
- All functionality works by keyboard. Maintain logical tab order, visible `:focus-visible`
  ring, Escape behavior for dismissible overlays and correct focus return.
- Use semantic headings, landmarks, labels, table semantics and status announcements.
  Do not use a clickable `div` where a button/link is appropriate.
- Respect `prefers-reduced-motion` by removing nonessential movement and transitions;
  retain state feedback without animation.
- Respect `prefers-reduced-transparency` with opaque surfaces. Also support system increased
  contrast where exposed; do not infer it from color scheme.
- Minimum usable hit area is 24×24 CSS px with spacing; aim for 44 px on primary actions.
- Never rely on blur, color, sound, animation or icon-only meaning to explain a safety state.

## Window and layout behavior

The current Tauri shell declares a 680×520 px minimum and 1100×760 px initial size. Treat
680×520 as the current supported lower bound, not a validated final design decision. At
narrow widths, reduce sidebar width or provide a deliberate collapsible/navigation mode;
do not crop data or force horizontal scrolling of the entire app. Tables may scroll within
their own region with pinned essential identity/action columns. At larger sizes, constrain
reading width and let data regions use available space. Support macOS full-screen and
resizing without losing navigation or dialog context.

## Data visualization

Use a chart only when it reveals a comparison or trend that a number/list cannot. Label
units, time range, source and freshness; provide an accessible textual summary and a table
or equivalent for exact values. Use stable color mapping, direct labels and restrained axes.
Never imply unsupported savings, performance, battery or health improvements. Storage totals
must state scope and account for unknown/unreadable data visibly.

## Safety-critical presentation

For every recommendation, show why it appeared, evidence and freshness, uncertainty/risk,
policy eligibility, exact planned effect and recovery limits. Keep “risk” separate from
“allowed.” Approval must refer to the exact previewed items and effects and become stale if
identity, permission or policy changes. Explain partial failures. Quarantine, restore and
permanent deletion are distinct actions; permanent deletion gets its own explicit
confirmation. A “safe” or “reversible” claim is allowed only when the backend establishes
the relevant conditions and limitations.

## React, TypeScript, CSS and Tauri implementation

- Follow the existing React + TypeScript + CSS approach. Use semantic CSS variables for
  shared values; do not add a second styling framework or per-screen color palette.
- Keep UI state presentation-oriented. Product decisions, filesystem authority and safety
  policy belong in validated Rust command/domain boundaries, not React.
- Use native semantic elements, typed props, named exports and accessible state attributes.
  Do not make a frontend type assertion a substitute for runtime IPC validation.
- Avoid `backdrop-filter` as a claim of native Liquid Glass. Feature-detect it and preserve
  contrast without it. Any genuine native material work must be reviewed at the Tauri/macOS
  boundary and labeled as native integration.
- Keep animations short, purposeful and interruptible; honor user preferences in CSS and
  native overlays.

## Visual review and acceptance

Before accepting a meaningful UI change, inspect screenshots at the initial and minimum
window sizes, in light and dark appearance, and with reduced transparency/motion where
available. Check text/control contrast, clipping, focus order, keyboard operation, hover,
loading, empty, error and disabled states. For safety-related UI, review copy and actions
against product requirements and threat model. Attach screenshots to the PR when practical;
never fabricate a running-app screenshot.

## Anti-patterns

- Glass or blur on every panel; glass-on-glass; decorative glow or gradient fields.
- Giant rounded cards, dashboard wallpaper, excessive pills or dense tiny labels.
- Invented metrics, fake scan results, false “protected/safe” claims or mock data presented
  as live system state.
- Destructive action on hover, row selection, opening a context menu, or a default button.
- Color-only risk/status, low-contrast muted text, hidden focus or hover-only controls.
- Tables and settings on unstable translucent surfaces.
- Motion without purpose, motion that blocks action, or ignoring reduced-motion settings.
- Hard-coded colors in screen styles, inconsistent spacing, multiple competing token sets.
- Calling CSS blur “Liquid Glass” or implying Apple affiliation/endorsement.
