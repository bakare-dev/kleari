# ADR-008: Native window material and unified title bar

- **Status:** Accepted
- **Context:** The CSS sidebar blur only blurred Kleari's own flat canvas, so the app did not
  feel like current macOS. The product owner asked for the native glass look. THEME.md and
  DESIGN.md require genuine material to come from explicit macOS integration, be reviewed at
  the Tauri/macOS boundary and be described accurately.
- **Decision:** Use Tauri's built-in window effects, with no new dependency or custom native
  code:
  - The main window is `transparent` with `windowEffects` `["liquidGlassRegular", "sidebar"]`.
    On macOS 26 and later this is Apple's Liquid Glass (`NSGlassEffectView`). On macOS 14–15
    Tauri falls back to the `sidebar` vibrancy material.
    `state: "followsWindowActiveState"` applies only to that vibrancy fallback; Tauri ignores
    `state` for Liquid Glass.
  - The title bar uses `titleBarStyle: "Overlay"` with a hidden title, so the sidebar runs
    under the window controls. A transparent, control-free strip with `data-tauri-drag-region`
    moves the window; this needs `core:window:allow-start-dragging`.
  - The frontend marks the root with `data-native-window` when running in Tauri. CSS then
    makes the page and sidebar backgrounds transparent so the material shows through the
    sidebar only. Content, tables and settings stay on the opaque canvas, as THEME.md
    requires. In a plain browser the existing CSS sidebar remains as a fallback.
  - Regular rather than Clear glass, for the legibility of sidebar text and icons.
- **Consequences:**
  - The material is genuine native integration, not a CSS approximation. It adapts to the
    window's appearance (set from Settings via `setTheme`), and macOS makes it opaque when
    Reduce transparency is on.
  - Transparent windows rely on macOS private APIs that Tauri 2.12.1+ always enables. This
    matters for Mac App Store distribution, which is not a current channel. Re-check before
    any App Store submission.
  - With the overlay title bar, the window can be dragged only from the strip, and a known
    Tauri limitation prevents dragging while the window is unfocused. The title bar height
    differs between macOS versions, so the sidebar's top padding (52 px) needs checking on
    each supported version.
  - Web-rendered sidebar text is not "vibrant" the way native labels are. Primary text is
    used for navigation to keep contrast over varied backgrounds; it must be checked
    visually over light and dark wallpapers.
  - Automated tests cannot see the native material. Each change to it needs manual visual
    verification on macOS 26+ and on 14–15.
