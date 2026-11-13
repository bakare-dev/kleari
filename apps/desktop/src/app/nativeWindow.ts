// In the desktop app the window is transparent with a native glass material behind the web
// view (see docs/adr/ADR-008-native-window-material.md). CSS keys off `data-native-window` to
// let that material show through the sidebar while content stays on opaque surfaces. In a
// plain browser there is no native material, so the attribute is absent and the CSS
// fallback (an opaque or blurred sidebar) applies.

export const NATIVE_WINDOW_ATTRIBUTE = "data-native-window";

export function markNativeWindow(root: Pick<HTMLElement, "setAttribute" | "removeAttribute">, native: boolean): void {
  if (native) {
    root.setAttribute(NATIVE_WINDOW_ATTRIBUTE, "");
  } else {
    root.removeAttribute(NATIVE_WINDOW_ATTRIBUTE);
  }
}
