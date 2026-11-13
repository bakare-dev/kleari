# Kleari design documentation

This directory is the source of truth for Kleari's visual language and UI behaviour. It
describes intended product screens; it does not mean those screens or their product
capabilities are implemented. [THEME.md](THEME.md) is the authoritative product identity,
palette and typography reference. If a design document or existing style differs, follow
THEME.md and document a staged resolution rather than silently redesigning unrelated UI.

**Design v2:** the approved v2 package is in [`v2/`](v2/HANDOFF.md). It contains the
handoff, PNG references for every screen in light and dark, the interactive sources, the
tokens and the brand assets. It is being implemented in steps, and THEME.md, DESIGN.md and
SCREENS.md are updated as each step lands. Until a screen is restyled, the app keeps the
pre-v2 layout for it.

## Read in this order

1. [THEME.md](THEME.md) — authoritative visual identity, palette, type, layout and safety expression.
2. [DESIGN.md](DESIGN.md) — supporting material, component, accessibility and implementation rules.
3. [SCREENS.md](SCREENS.md) — screen purpose, hierarchy, states and primary actions.
4. [COMPONENTS.md](COMPONENTS.md) — reusable component responsibilities and boundaries.
5. [INTERACTIONS.md](INTERACTIONS.md) — navigation, input, feedback and safety flows.
6. [ASSETS.md](ASSETS.md) — approved references, icons, imagery and screenshot practices.

Read the screen-specific sections plus the component and interaction guidance relevant to
your change. Update this contract when an approved design decision changes. Project safety
and architecture rules in [CONTRIBUTING.md](../../CONTRIBUTING.md), product requirements and the threat
model remain authoritative for capability and safety.
