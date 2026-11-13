// Appearance preference: System (default), Light or Dark. Stored per Mac in WebView
// localStorage until the planned SQLite settings store (ADR-006) exists.

export type AppearancePreference = "system" | "light" | "dark";

export const APPEARANCE_STORAGE_KEY = "kleari.appearance";
export const appearanceOptions: readonly AppearancePreference[] = ["system", "light", "dark"];

type ReadableStorage = Pick<Storage, "getItem">;
type WritableStorage = Pick<Storage, "setItem">;

function isAppearance(value: unknown): value is AppearancePreference {
  return appearanceOptions.some((option) => option === value);
}

/**
 * Where the current preference came from, so Settings can describe it accurately:
 * - `saved`: read from, or just written to, storage
 * - `default`: nothing saved yet
 * - `unreadable`: storage missing or reading it threw
 * - `invalid`: an unrecognized saved value was ignored
 * - `saveFailed`: the latest choice applies now but could not be written
 */
export type AppearanceStorageStatus = "saved" | "default" | "unreadable" | "invalid" | "saveFailed";

export interface LoadedAppearance {
  preference: AppearancePreference;
  status: Exclude<AppearanceStorageStatus, "saveFailed">;
}

/**
 * Reads the saved preference. Missing storage, a storage error (for example when WebView
 * storage is disabled) or an unknown value all fall back to "system", the documented
 * default, and the status says which case occurred; a stale or tampered value is never
 * applied.
 */
export function loadAppearance(storage: ReadableStorage | undefined): LoadedAppearance {
  if (storage === undefined) {
    return { preference: "system", status: "unreadable" };
  }
  let value: string | null;
  try {
    value = storage.getItem(APPEARANCE_STORAGE_KEY);
  } catch {
    // Reported via the status, not swallowed: Settings tells the user it couldn't be read.
    return { preference: "system", status: "unreadable" };
  }
  if (value === null) {
    return { preference: "system", status: "default" };
  }
  return isAppearance(value) ? { preference: value, status: "saved" } : { preference: "system", status: "invalid" };
}

/** Returns whether the preference was saved, so the UI can say when it was not. */
export function saveAppearance(storage: WritableStorage | undefined, preference: AppearancePreference): boolean {
  if (storage === undefined) {
    return false;
  }
  try {
    storage.setItem(APPEARANCE_STORAGE_KEY, preference);
    return true;
  } catch {
    // Reported to the caller via the return value, not swallowed.
    return false;
  }
}

/**
 * Applies the preference to the page. "system" removes the override so CSS follows
 * `prefers-color-scheme`; light and dark set `data-theme` on the root element.
 */
export function applyAppearanceToDocument(
  root: Pick<HTMLElement, "setAttribute" | "removeAttribute">,
  preference: AppearancePreference,
): void {
  if (preference === "system") {
    root.removeAttribute("data-theme");
  } else {
    root.setAttribute("data-theme", preference);
  }
}

/** The native window theme matching a preference: `null` follows macOS. */
export function windowThemeFor(preference: AppearancePreference): "light" | "dark" | null {
  return preference === "system" ? null : preference;
}

/** Safely returns `window.localStorage`, which can throw when storage is disabled. */
export function browserStorage(): Storage | undefined {
  try {
    return typeof window === "undefined" ? undefined : window.localStorage;
  } catch {
    // Deliberate: no storage means the preference cannot persist; callers handle undefined.
    return undefined;
  }
}
