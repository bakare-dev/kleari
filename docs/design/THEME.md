# Kleari product theme

**Authority:** this is the canonical starting theme for every Kleari screen and surface.
It describes intended product design, not implemented functionality. The palette values
are the product owner's requested starting values. They are Kleari decisions, not Apple
specifications.

## Product identity

Kleari is a premium, restrained, macOS-inspired maintenance and developer productivity
utility. It should feel native-inspired, polished, professional, calm, precise, trustworthy
and easy to navigate. Give developers useful system information without visual noise.
Keep the interface consistent across the main window, menu-bar popover, contextual menus,
dialogs and notifications.

The visual language supports the product principle: **Scan freely. Explain clearly. Execute
only with explicit authorisation.** It must not pressure users into risky actions or obscure
their consequences. Do not use generic SaaS dashboard styling, excessive gradients, neon
cyber-security motifs, unnecessary card grids, decorative animation or one-off component
styles.

## Canonical semantic palette

**Design v2 (2026-10-18).** The product owner approved the v2 design in
[`v2/`](v2/HANDOFF.md). Its glass surfaces, colourful icon tiles and Navigator logo are a newer
decision than the restraint guidance elsewhere in this file; where they differ, v2 wins and
this file is updated as each part ships. Safety rules in CONTRIBUTING.md still override the design.

Every colour value lives in `--mp-*` tokens in
[`v2/tokens.css`](v2/tokens.css), which is copied verbatim to `apps/desktop/src/app/tokens.css`.
A test keeps the two files identical. Change values in the design package first.
Components must not scatter literal colours across screens.

The app supports System, Light and Dark appearance; System is the default. Settings →
Appearance implements the choice. System leaves `:root` without `data-theme`, so
`prefers-color-scheme` applies; Light and Dark set `data-theme="light"` or `"dark"`.
`tokens.css` declares its dark values twice, for system Dark Mode and explicit Dark. A test
checks the two blocks are identical and cover every light token.

`styles.css` keeps its semantic `--color-*` roles as aliases of the v2 tokens, so they switch
with the theme. New and restyled components use the `--mp-*` tokens directly. The aliases go
once nothing references them.

| Role | v2 token | Light | Dark | Use |
|---|---|---|---|---|
| `--color-background` | `--mp-bg` | `#F5F7FA` | `#14181E` | Main window canvas |
| `--color-surface` | `--mp-card` | `#FFFFFF` | `#1C222B` | Cards and stable surfaces |
| `--color-surface-secondary` | `--mp-seg-bg` | `#E9EDF3` | `#252D38` | Hover, pressed and segmented fills |
| `--color-text-primary` | `--mp-text` | `#182230` | `#F0F3F7` | Main content and headings |
| `--color-text-secondary` | `--mp-text-2` | `#5D6878` | `#B5BFCC` | Supporting content |
| `--color-border` | `--mp-border` | 7% ink | 6% white | Card edges and separators (decorative only) |
| `--color-border-strong` | `--mp-strong` | `#C5CED9` | `#465364` | Stronger dividers |
| `--color-track` | `--mp-track` | `#EDF0F5` | `#2A323E` | Meter and progress tracks |
| `--color-accent` | `--mp-accent` | `#2878A8` | `#69B7E4` | Primary emphasis and selected state |
| `--color-accent-hover` | `--mp-accent-text` | `#21658E` | `#8BC9EC` | Hovered accent action, accent text |
| `--color-accent-subtle` | `--mp-accent-soft` | `#E8F2F8` | `#233B4A` | Selected or lightly emphasized surface |
| `--color-success` | `--mp-ok-text` | `#15703F` | `#68C995` | Confirmed success |
| `--color-warning` | `--mp-warn-text` | `#8A5600` | `#F0BD63` | Caution and uncertainty |
| `--color-danger` | `--mp-danger-text` | `#B42332` | `#FF7D86` | Failure or destructive action |
| `--color-sidebar` | `--mp-glass` | 62% light glass | 55% dark glass | Browser-fallback sidebar |
| `--shadow-card` | `--mp-card-shadow` | soft two-layer | none | Cards (dark relies on the border) |
| `--shadow-popover` / `--shadow-dialog` | `--mp-menu-shadow` / `--mp-sheet-shadow` | | | Popovers and dialogs |

v2 defines no tertiary text colour. `--color-text-tertiary` keeps `#7A8594` / `#929EAE`,
declared in `styles.css` with the other non-v2 roles.

**Contrast checks for the v2 values** (WCAG 2.2):
- Success text: 6.14:1 light on card, 5.72:1 on the canvas.
- Warning text: 6.16:1 light on card, 5.74:1 on the canvas.
- Pressed secondary button (`--mp-accent-text` on `--mp-seg-bg`): 5.39:1 light, 7.72:1 dark.

`--mp-border` is translucent and far below 3:1. Use it for decorative edges only. Form-field
borders keep `--color-text-tertiary`, because `--mp-strong` on a card is only 1.59:1.

The v2 package lists a few tokens that differ slightly between screens:
`borderSolid`, `scrim`, `menuBg`, `menuShadow` and `warnBg`. `tokens.css` keeps the Dashboard
value for each, and the app uses those.

`--color-text-tertiary` light value `#7A8594` against the specified background measures
about 3.49:1, below the 4.5:1 target for normal text. Keep it to nonessential large text
or decorative annotation until a contrast-safe value is approved; do not use it for small
supporting copy. Do not silently change the requested palette. Review any token adjustment
with the product owner and verify the final rendered pairings. Status/risk must include a
text label or icon and must never rely on color alone.

Additional semantic tokens for focus, control text, elevated surfaces or selected controls
must be derived from these palette roles, checked for contrast, and documented here before
being shared across screens. Do not create a second palette or use a literal color as a
screen-specific exception.

## Typography

Use the native-inspired system stack:

```css
font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
```

Use the system monospace stack for file paths, hashes and technical identifiers only.
Keep weights and hierarchy consistent, avoid ultra-light text, and use tabular numerals for
aligned measurements where available.

| Role | Size |
|---|---|
| Page title | 24–28 px |
| Section heading | 15–17 px |
| Body | 13–14 px |
| Supporting label | 11–12 px; meet text contrast before use |
| Important metric | 24–32 px |
| Dense table | 12–13 px, with readable row height |

## Layout, spacing and components

- Use an 8 px base rhythm, with 4 px adjustments where alignment requires them. Keep
  spacing values on a documented scale (4, 8, 12, 16, 24, 32, 40 px).
- Align content to a consistent page grid. Preserve a usable reading width and let data
  regions use remaining space; resizing must not clip essential controls or information.
- Use restrained 8–12 px radii for standard surfaces. Avoid nested rounded containers.
- Prefer subtle borders and clear spacing before shadows. Use elevation only to explain a
  real layer relationship.
- Use one consistent line-icon family with matching optical sizes/stroke weights. Label
  icon-only controls accessibly.
- Primary buttons are visually clear and use an action verb. Secondary actions are quieter.
  Destructive actions are distinct, explicit and never the default action.
- Tables, scan results, file inventories, forms and history use stable opaque surfaces.
  Align numeric columns and label units, source and freshness.
- Charts need a clear comparison/trend purpose, text alternatives and exact values. Never
  imply savings or performance benefits unsupported by measurements.
- Define hover, visible focus, pressed, selected, disabled, loading, success and error
  states. State is not communicated by hue alone.

Detailed visual behavior and material implementation are in [DESIGN.md](DESIGN.md);
screen, component and interaction specifics remain in their corresponding specifications.

## Selective Liquid Glass-inspired materials

Use translucency selectively for the menu-bar popover, compact floating utility panels,
contextual menus and transient controls where it improves hierarchy. Use stable opaque
surfaces for main-page backgrounds, tables, inventories, scan results, settings, activity
history and safety-critical confirmations.

Avoid stacked glass, heavy blur, glowing borders, unreadable text over changing backgrounds
and nested translucent layers. Provide opaque fallbacks for reduced transparency and when
contrast cannot be maintained. Do not place dense or moving content behind critical text.

CSS opacity and `backdrop-filter` in Tauri are visual approximations, not Apple's genuine
native Liquid Glass material. Do not claim equivalence or Apple affiliation. A native
material requires explicit macOS integration and separate technical validation.

**Implemented native material ([ADR-008](../adr/ADR-008-native-window-material.md)):** the
desktop window uses Tauri's native window effects. That means Liquid Glass Regular on
macOS 26+, falling back to sidebar vibrancy on macOS 14–15. The overlay title bar lets the
sidebar run under the window controls. Only the sidebar is transparent to the material; the
content canvas stays opaque. The CSS blur above remains the fallback when the page runs
outside the desktop app.

## Screen visual expectations

These are design directions, not evidence that the screens or capabilities exist.

1. **Overview/dashboard:** show freshness before metrics; group useful readings without a
   wall of cards; distinguish unavailable values from zero.
2. **Storage analysis:** communicate scope and unknown coverage; use an opaque breakdown
   and readable inventory.
3. **Cleanup and scan results:** make read-only scan status, evidence, uncertainty, risk and
   policy eligibility distinct; selection only enters a review.
4. **Developer Tools:** group findings by recognizable tool/artifact and explain the
   classification without implying every path is safe.
5. **System monitoring:** show units, source and freshness; never draw fabricated history.
6. **Quarantine and recovery:** show state, limits and conflicts; distinguish restore from
   permanent deletion.
7. **Activity history:** use a stable chronological list, privacy-aware path detail and
   honest partial outcomes.
8. **Settings:** use stable forms, clear groups and explicit permission state.
9. **Menu-bar popover:** compact status and a route to the main app; limited translucency;
   no destructive quick action.

See [SCREENS.md](SCREENS.md) for first-view hierarchy, actions, empty states, errors and
screen behavior.

## Accessibility and platform behavior

- Support system, light and dark appearances with consistent semantic meaning.
- Meet WCAG 2.2 AA contrast: 4.5:1 for normal text, 3:1 for large text and meaningful
  non-text boundaries/indicators. Test actual foreground/background pairs, including accent
  buttons, secondary surfaces and translucency. The palette's tertiary light text exception
  above must not be used for small text.
- Make navigation and controls keyboard accessible with logical order and visible focus.
- Use semantic controls, accessible names, correct table/heading/status semantics and
  announce meaningful progress and outcomes.
- Respect reduced motion by removing nonessential movement; retain state feedback.
- Respect reduced transparency with opaque surfaces. Support increased contrast where the
  platform exposes it.
- Distinguish estimates from measured values and show units, source and freshness.
- Provide honest empty, loading, error, stale and permission-denied states.

## Trust and safety

Never present fabricated system metrics, storage savings, scan progress or historical trends
as real data. Make uncertainty visible. Explain why an item was found, the exact planned
effect, what approval authorizes, whether recovery is possible and its limitations. A visual
risk label does not establish backend eligibility or safety.

Keep scan, preview, explicit approval, quarantine, restore and permanent deletion as distinct
steps. Permanent deletion needs its own explicit confirmation. Never style an action to
pressure, mislead or obscure consequences.

## Implementation status and derived tokens

`apps/desktop/src/app/tokens.css` and `styles.css` implement the palette above for light and
dark appearances.

**v2 interface.** The v2 shell (232 px glass sidebar, Navigator icon, wordmark and tagline,
offline chip), shared components and every screen are built in `apps/desktop/src/v2/` on
the `--mp-*` tokens. The accent palette lives in `v2.css` as `.tone-*` gradient pairs. In
the desktop app the sidebar shows the native window material (ADR-008). Release builds
show the screens wired to real data (Storage, System, Developer, Processes and appearance
Settings); development builds also show the remaining screens with sample data (see
[SCREENS.md](SCREENS.md#design-v2-implementation-status)).
The app icon is the v2 Navigator mark in every build. The derived roles
below follow the rule for additional tokens and were contrast-checked against their rendered
pairs:

| Token | Derivation | Use and contrast notes |
|---|---|---|
| `--color-accent-contrast` | Light `#FFFFFF`, dark `--color-background` | Glyphs on accent tiles: 4.84:1 light, 8.05:1 dark |
| `--color-focus` | `--color-accent` | Focus ring: 4.84:1 light, 7.23:1 dark on surface |
| `--color-sidebar-opaque` | `--color-surface-secondary` mixed with `--color-accent-subtle` (dark: `--color-surface` 80% + accent-subtle) | Opaque sidebar fallback |
| `--color-sidebar` | `--mp-glass` with backdrop blur | Browser fallback sidebar; opaque under reduced transparency or without `backdrop-filter`. In the desktop app the sidebar is transparent to the native material instead (ADR-008) |

`--color-accent` text on `--color-accent-subtle` measures 4.26:1 in light mode, so selected
navigation uses primary text on the subtle fill with an accent icon only. Secondary
buttons pair each text color with a passing background: resting `--color-accent` on
`--color-surface` (4.84:1 light, 7.23:1 dark); hover `--color-accent-hover` on
`--color-accent-subtle` (5.57:1 light, 6.49:1 dark); pressed `--color-accent-hover` on
`--color-surface-secondary` (5.39:1 light, 7.72:1 dark). Do not use `--color-accent` itself on the
subtle fill for text. In the desktop app the sidebar shows the native window material
(ADR-008). Because the material behind it varies, navigation text there uses
`--color-text-primary`, and legibility must be checked visually over light and dark
wallpapers; it cannot be computed. The app icon (`apps/desktop/src-tauri/icons/kleari.svg`) is the v2
Navigator mark; see [ASSETS.md](ASSETS.md).
