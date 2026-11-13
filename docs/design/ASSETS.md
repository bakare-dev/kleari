# Design assets and references

## Current repository assets

The repository currently contains Tauri application icons at
`apps/desktop/src-tauri/icons/icon.png`, `apps/desktop/src-tauri/icons/icon.icns` and the
editable source `apps/desktop/src-tauri/icons/kleari.svg`. They are the v2 **Navigator**
mark from [`v2/brand/`](v2/brand/): an orbit ellipse, a pilot arrow and a dot on a
`#6CC4FF` → `#2F6FED` → `#6D3FE0` squircle. Regenerate the PNG and ICNS from
`v2/brand/png/icon_1024.png` with `pnpm tauri icon <file> -o <temporary directory>` and copy
only the files `tauri.conf.json` references. These are app identity assets, not UI mockups
or screenshots. Brand rules: don't recolour the icon; below 32 px use it without its shadow;
in the menu bar use the one-colour template glyph (`v2/brand/menubar-glyph-template.svg`).

`docs/design/references/kleari-ui-showcase-exploratory.png` is a generated, exploratory
multi-screen mockup supplied by the product owner. It sets the visual direction (light
canvas, white cards, blue accent, frosted sidebar) but is not an implementation
specification or proof of working UI: most screens and the battery, sparkline and process
data it shows are not implemented, and THEME.md remains authoritative for palette and
contrast.

## Locations and naming

Store approved design references under `docs/design/references/` and final product UI
assets under `apps/desktop/src/assets/` (create these directories only when a real asset is
added). Use descriptive lowercase kebab-case names and include screen/state and theme when
relevant, for example `cleanup-review-dark-approved.png`. Preserve editable source
alongside exports when licensing permits. Do not invent a path for an asset that does not
exist.

## Icons

Prefer macOS system symbols through a supported, licensed symbol source or small bespoke
SVGs that match product stroke and size rules. Confirm symbol licensing and availability for
the minimum OS before shipping. Use a text label/accessibility name for icon-only controls;
do not use emoji as final interface icons. Do not use Apple logos or imply Apple endorsement.

## Images, illustrations and mockups

Use images only when they explain a real concept or add necessary context. Prefer simple
product-native diagrams over decorative stock art. Generated mockups are exploratory
references only and must be labeled as such; they are not implementation specifications or
proof of working UI. Final product assets require human review for accuracy, rights,
accessibility and consistency.

## Screenshots and reference updates

Capture screenshots from the actual running application when available; never fabricate or
retouch a screenshot to suggest implemented behavior. Include OS/theme, window size and
state in the filename or adjacent note. When a design changes, update or remove superseded
references in the same review, identify the newly approved direction and keep links in the
relevant specification current. Do not check in personal file paths, account details,
notifications or other private desktop content.

External guidance is reference material, not a claim of conformance or affiliation:

- [Apple: Adopting Liquid Glass](https://developer.apple.com/documentation/TechnologyOverviews/adopting-liquid-glass)
- [Apple Human Interface Guidelines: Materials](https://developer.apple.com/design/human-interface-guidelines/materials)
- [Apple WWDC25: Meet Liquid Glass](https://developer.apple.com/videos/play/wwdc2025/219/)

The web-rendered Tauri UI can use CSS opacity/blur as a visual approximation only. Apple
describes Liquid Glass as an adaptive system material with platform behavior; CSS blur is
not equivalent. Kleari's token, contrast, layout and use-case decisions are Kleari
proposals informed by these references.
