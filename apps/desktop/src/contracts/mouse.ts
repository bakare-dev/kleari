// IPC contract for the Mouse page. Mirrors src-tauri/src/mouse/{mod,config}.rs; every
// response is validated before the UI sees it.

import { ContractError, nonNegativeInteger, oneOf, record } from "./systemMetrics";

/** Wheel speeds offered, in percent of macOS's own. */
export const mouseSpeeds = [50, 75, 100, 150, 200, 300] as const;
/** Buttons that can be changed, as macOS numbers them (2 is the middle button). */
export const mouseButtons = [2, 3, 4, 5, 6, 7] as const;
/** Left and right click (0 and 1): part of a combination only, with one of `mouseButtons`. */
export const clickButtons = [0, 1] as const;
/** Every button a combination can use. */
export const comboButtonNumbers = [...clickButtons, ...mouseButtons] as const;
export const isClick = (button: number): boolean => button === 0 || button === 1;
/** Keys a shortcut can use (the backend's `KEYS`). */
export const shortcutKeys = [
  ..."abcdefghijklmnopqrstuvwxyz0123456789",
  "=", "-", "[", "]",
  "return", "tab", "space", "delete", "escape",
  "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
  "left", "right", "down", "up",
] as const;
export type ShortcutKey = (typeof shortcutKeys)[number];

export interface Modifiers {
  command: boolean;
  shift: boolean;
  option: boolean;
  control: boolean;
}

export interface Shortcut {
  key: ShortcutKey;
  modifiers: Modifiers;
}

export const simpleButtonActions = [
  "back",
  "forward",
  "missionControl",
  "appWindows",
  "previousSpace",
  "nextSpace",
  "middleClick",
  "disabled",
  "playPause",
  "nextTrack",
  "previousTrack",
  "volumeUp",
  "volumeDown",
  "mute",
] as const;
export type SimpleButtonAction = (typeof simpleButtonActions)[number];
export type ButtonAction = { kind: SimpleButtonAction } | { kind: "shortcut"; shortcut: Shortcut };

export interface ButtonMapping {
  button: number;
  action: ButtonAction;
}

export const scrollActions = ["switchSpaces", "zoom", "switchTabs", "horizontalScroll"] as const;
export type ScrollAction = (typeof scrollActions)[number];

/** What a combination does: while held, each wheel step does `wheel`; or, pressed together, `action` once. */
export type ComboAction = { kind: "wheel"; wheel: ScrollAction } | { kind: "press"; action: ButtonAction };

/** Up to two of buttons 3–8 (macOS numbers 2–7, in increasing order) and modifier keys, held together. */
export interface Combo {
  buttons: number[];
  modifiers: Modifiers;
  does: ComboAction;
}

/** At most this many buttons in one combination. */
export const MAX_COMBO_BUTTONS = 2;

export interface MouseSettings {
  enabled: boolean;
  reverseVertical: boolean;
  reverseHorizontal: boolean;
  speed: number;
  smooth: boolean;
  buttons: ButtonMapping[];
  combos: Combo[];
}

const accesses = ["allowed", "notAllowed", "unsupported"] as const;
export type MouseAccess = (typeof accesses)[number];

const sources = ["saved", "default", "invalid", "unavailable"] as const;

export interface MouseStatus {
  settings: MouseSettings;
  source: (typeof sources)[number];
  access: MouseAccess;
  /** The settings are being applied to the mouse now. */
  active: boolean;
}

export const MAX_COMBOS = 8;

function boolean(value: unknown, field: string): boolean {
  if (typeof value !== "boolean") {
    throw new ContractError(field);
  }
  return value;
}

function number<T extends number>(allowed: readonly T[], value: unknown, field: string): T {
  const found = allowed.find((entry) => entry === value);
  if (found === undefined) {
    throw new ContractError(field);
  }
  return found;
}

function list(value: unknown, max: number, field: string): unknown[] {
  if (!Array.isArray(value) || value.length > max) {
    throw new ContractError(field);
  }
  return value;
}

function modifiers(value: unknown, field: string): Modifiers {
  const fields = record(value, field);
  return {
    command: boolean(fields.command, `${field}.command`),
    shift: boolean(fields.shift, `${field}.shift`),
    option: boolean(fields.option, `${field}.option`),
    control: boolean(fields.control, `${field}.control`),
  };
}

function buttonAction(value: unknown, field: string): ButtonAction {
  const fields = record(value, field);
  if (fields.kind === "shortcut") {
    const shortcut = record(fields.shortcut, `${field}.shortcut`);
    return {
      kind: "shortcut",
      shortcut: { key: oneOf(shortcutKeys, shortcut.key, `${field}.shortcut.key`), modifiers: modifiers(shortcut.modifiers, `${field}.shortcut.modifiers`) },
    };
  }
  return { kind: oneOf(simpleButtonActions, fields.kind, `${field}.kind`) };
}

function comboAction(value: unknown, field: string): ComboAction {
  const fields = record(value, field);
  if (fields.kind === "wheel") {
    return { kind: "wheel", wheel: oneOf(scrollActions, fields.wheel, `${field}.wheel`) };
  }
  if (fields.kind === "press") {
    return { kind: "press", action: buttonAction(fields.action, `${field}.action`) };
  }
  throw new ContractError(`${field}.kind`);
}

function combo(value: unknown, field: string): Combo {
  const fields = record(value, field);
  const buttons = list(fields.buttons, MAX_COMBO_BUTTONS, `${field}.buttons`).map((button, index) => number(comboButtonNumbers, button, `${field}.buttons[${index}]`));
  // In increasing order; left or right click only with one of the other buttons, never both.
  const clicks = buttons.filter((button) => isClick(button)).length;
  if (buttons.some((button, index) => index > 0 && button <= buttons[index - 1]) || (clicks > 0 && (clicks > 1 || buttons.length < MAX_COMBO_BUTTONS))) {
    throw new ContractError(`${field}.buttons`);
  }
  return { buttons, modifiers: modifiers(fields.modifiers, `${field}.modifiers`), does: comboAction(fields.does, `${field}.does`) };
}

export function parseMouseSettings(value: unknown, field = "settings"): MouseSettings {
  const fields = record(value, field);
  return {
    enabled: boolean(fields.enabled, `${field}.enabled`),
    reverseVertical: boolean(fields.reverseVertical, `${field}.reverseVertical`),
    reverseHorizontal: boolean(fields.reverseHorizontal, `${field}.reverseHorizontal`),
    speed: number(mouseSpeeds, fields.speed, `${field}.speed`),
    smooth: boolean(fields.smooth, `${field}.smooth`),
    buttons: list(fields.buttons, mouseButtons.length, `${field}.buttons`).map((entry, index) => {
      const mapping = record(entry, `${field}.buttons[${index}]`);
      return {
        button: number(mouseButtons, mapping.button, `${field}.buttons[${index}].button`),
        action: buttonAction(mapping.action, `${field}.buttons[${index}].action`),
      };
    }),
    combos: list(fields.combos, MAX_COMBOS, `${field}.combos`).map((entry, index) => combo(entry, `${field}.combos[${index}]`)),
  };
}

/** What the button finder reports while the Mouse page is open (src-tauri/src/mouse/probe.rs). */
export type Probe =
  | { kind: "down" | "up"; button: number }
  | { kind: "scroll"; vertical: -1 | 0 | 1; horizontal: -1 | 0 | 1; stepped: boolean };

const steps = [-1, 0, 1] as const;
/** macOS numbers up to 31 buttons; more is malformed. */
const MAX_BUTTON = 31;

export function parseProbe(payload: unknown): Probe {
  const fields = record(payload, "$");
  if (fields.kind === "down" || fields.kind === "up") {
    const button = nonNegativeInteger(fields.button, "button");
    if (button > MAX_BUTTON) {
      throw new ContractError("button");
    }
    return { kind: fields.kind, button };
  }
  if (fields.kind === "scroll") {
    return {
      kind: "scroll",
      vertical: number(steps, fields.vertical, "vertical"),
      horizontal: number(steps, fields.horizontal, "horizontal"),
      stepped: boolean(fields.stepped, "stepped"),
    };
  }
  throw new ContractError("kind");
}

export function parseMouseStatus(payload: unknown): MouseStatus {
  const fields = record(payload, "$");
  return {
    settings: parseMouseSettings(fields.settings),
    source: oneOf(sources, fields.source, "source"),
    access: oneOf(accesses, fields.access, "access"),
    active: boolean(fields.active, "active"),
  };
}
