import {
  MAX_COMBOS,
  comboButtonNumbers,
  isClick,
  mouseButtons,
  mouseSpeeds,
  scrollActions,
  shortcutKeys,
  type ButtonAction,
  type Combo,
  type Modifiers,
  type MouseSettings,
  type MouseStatus,
  type Probe,
  type ScrollAction,
  type Shortcut,
  type ShortcutKey,
  type SimpleButtonAction,
} from "../../contracts/mouse";
import type { LoginItem } from "../../contracts/settings";
import type { MouseChange } from "./mouseSaves";

/** What each changeable button is called. macOS numbers buttons from 0; people count from 1. */
export const buttonNames: Record<number, { label: string; sub: string }> = {
  2: { label: "Middle button", sub: "Pressing the wheel, or a third button" },
  3: { label: "Button 4", sub: "Usually the rear side button (Back)" },
  4: { label: "Button 5", sub: "Usually the front side button (Forward)" },
  5: { label: "Button 6", sub: "On mice with more buttons" },
  6: { label: "Button 7", sub: "On mice with more buttons" },
  7: { label: "Button 8", sub: "On mice with more buttons" },
};

/** The Select value for a button: its action, `usual` when unchanged. */
export type ActionChoice = "usual" | SimpleButtonAction | "shortcut";

export const actionOptions: readonly { value: ActionChoice; label: string }[] = [
  { value: "usual", label: "Its usual click" },
  { value: "back", label: "Back (⌘[)" },
  { value: "forward", label: "Forward (⌘])" },
  { value: "missionControl", label: "Mission Control (⌃↑)" },
  { value: "appWindows", label: "App windows (⌃↓)" },
  { value: "previousSpace", label: "Previous Space (⌃←)" },
  { value: "nextSpace", label: "Next Space (⌃→)" },
  { value: "lastApp", label: "Last app used (⌘⇥)" },
  { value: "nextWindow", label: "Next window of the app (⌘`)" },
  { value: "middleClick", label: "Middle click" },
  { value: "playPause", label: "Play / Pause" },
  { value: "nextTrack", label: "Next track" },
  { value: "previousTrack", label: "Previous track" },
  { value: "volumeUp", label: "Volume up" },
  { value: "volumeDown", label: "Volume down" },
  { value: "mute", label: "Mute" },
  { value: "disabled", label: "Do nothing" },
  { value: "shortcut", label: "Keyboard shortcut…" },
];

export const speedOptions = mouseSpeeds.map((speed) => ({ value: String(speed), label: speed === 100 ? "100% (macOS)" : `${speed}%` }));

export const scrollActionNames: Record<ScrollAction, string> = {
  switchSpaces: "Switch Spaces",
  zoom: "Zoom in and out",
  switchTabs: "Switch tabs",
  horizontalScroll: "Scroll sideways",
  switchApps: "Switch apps (like ⌘⇥; let go to choose)",
  switchWindows: "Switch the app’s windows (⌘`)",
};

export const scrollActionOptions = scrollActions.map((action) => ({ value: action, label: scrollActionNames[action] }));

export function actionFor(settings: MouseSettings, button: number): ButtonAction | null {
  return settings.buttons.find((mapping) => mapping.button === button)?.action ?? null;
}

export function choiceOf(action: ButtonAction | null): ActionChoice {
  return action === null ? "usual" : action.kind;
}

/** `settings` with `button` doing `action`, or its usual click for `null`. */
export function withButton(settings: MouseSettings, button: number, action: ButtonAction | null): MouseSettings {
  const others = settings.buttons.filter((mapping) => mapping.button !== button);
  const buttons = action === null ? others : [...others, { button, action }].sort((a, b) => a.button - b.button);
  return { ...settings, buttons };
}

/** Actions a combination pressed together can do (no "usual click": it has none). */
export const pressActionOptions = actionOptions.filter((option) => option.value !== "usual");

/** The Select value for one of a combination's two button slots: `none` or the button's number. */
export const comboButtonOptions = [{ value: "none", label: "No button" }, ...comboButtonNumbers.map((button) => ({ value: String(button), label: buttonLabel(button) }))];

export const noModifiers: Modifiers = { command: false, shift: false, option: false, control: false };

/** The modifier keys, in macOS's order, for a combination's key toggles. */
export const modifierKeys: readonly { key: keyof Modifiers; symbol: string; name: string }[] = [
  { key: "control", symbol: "⌃", name: "Control" },
  { key: "option", symbol: "⌥", name: "Option" },
  { key: "shift", symbol: "⇧", name: "Shift" },
  { key: "command", symbol: "⌘", name: "Command" },
];

const anyModifier = (modifiers: Modifiers) => modifiers.command || modifiers.shift || modifiers.option || modifiers.control;
const sameModifiers = (a: Modifiers, b: Modifiers) => a.command === b.command && a.shift === b.shift && a.option === b.option && a.control === b.control;

/** A combination's buttons from its two slots: unique, in increasing order, `none` left out. */
export function comboButtons(first: string, second: string): number[] {
  const chosen = [first, second].flatMap((value) => comboButtonNumbers.filter((button) => String(button) === value));
  return [...new Set(chosen)].sort((a, b) => a - b);
}

/** "Hold button 4 + button 5 + ⌘", as people read a combination. */
export function comboName(combo: Combo): string {
  const keys = modifierKeys.filter(({ key }) => combo.modifiers[key]).map(({ symbol }) => symbol).join("");
  const parts = [...combo.buttons.map((button) => buttonLabel(button).toLowerCase()), ...(keys ? [keys] : [])];
  return `${combo.does.kind === "wheel" ? "Hold" : "Press"} ${parts.join(" + ") || "nothing"}`;
}

/**
 * Why `combo` can't be saved among `others` (the other combinations), or `null` if it can.
 * The same rules as the backend's, said so the page can explain.
 */
export function comboProblem(combo: Combo, others: readonly Combo[]): string | null {
  const clicks = combo.buttons.filter(isClick).length;
  if (clicks > 1) {
    return "Choose left or right click, not both.";
  }
  if (clicks === 1 && combo.buttons.length < 2) {
    return `Add a second button: ${buttonLabel(combo.buttons[0]).toLowerCase()} works only with another button.`;
  }
  if (combo.does.kind === "wheel" && combo.buttons.length === 0 && !anyModifier(combo.modifiers)) {
    return "Choose a button or a key to hold.";
  }
  // Mirrors Rust: the switcher stays open while a button is held, and left click's release
  // must reach the app.
  if (combo.does.kind === "wheel" && combo.does.wheel === "switchApps" && (combo.buttons.length === 0 || combo.buttons.includes(0))) {
    return "Switching apps needs a mouse button to hold (not left click): letting go of it picks the app.";
  }
  if (combo.does.kind === "press" && combo.buttons.length === 0) {
    return "Choose the buttons to press together.";
  }
  if (combo.does.kind === "press" && combo.buttons.length === 1 && !anyModifier(combo.modifiers)) {
    return "Add a second button or a key: one button pressed alone is set under Buttons.";
  }
  const clash = others.some(
    (other) =>
      other.does.kind === combo.does.kind &&
      other.buttons.length === combo.buttons.length &&
      other.buttons.every((button, index) => button === combo.buttons[index]) &&
      sameModifiers(other.modifiers, combo.modifiers),
  );
  return clash ? "Another combination already uses these buttons and keys." : null;
}

/**
 * Adds a combination to start from: the first free one of a single button, then a single key,
 * then two buttons, held while scrolling to switch Spaces. Unchanged when the list is full or
 * none of these is free.
 */
export function withNewCombo(settings: MouseSettings): MouseSettings {
  if (settings.combos.length >= MAX_COMBOS) {
    return settings;
  }
  const wheel = (buttons: number[], modifiers: Modifiers): Combo => ({ buttons, modifiers, does: { kind: "wheel", wheel: "switchSpaces" } });
  const candidates: Combo[] = [
    ...mouseButtons.map((button) => wheel([button], noModifiers)),
    ...modifierKeys.map(({ key }) => wheel([], { ...noModifiers, [key]: true })),
    ...mouseButtons.flatMap((first) => mouseButtons.filter((second) => second > first).map((second) => wheel([first, second], noModifiers))),
  ];
  const free = candidates.find((candidate) => comboProblem(candidate, settings.combos) === null);
  return free === undefined ? settings : { ...settings, combos: [...settings.combos, free] };
}

/**
 * A combination's identity. Combinations are unique (`comboProblem`), so its content names it,
 * wherever it is in the list and whatever else changes meanwhile.
 */
export const comboKey = (combo: Combo): string => JSON.stringify(combo);

/** Adds a starting combination (`withNewCombo`), to the settings saved when it runs. */
export const addCombo: MouseChange = (settings) => {
  const next = withNewCombo(settings);
  return next === settings ? null : next;
};

/** Changes the combination `from` to `to`; doesn't apply once `from` is gone or if `to` would
 * repeat another combination. */
export function replaceCombo(from: Combo, to: Combo): MouseChange {
  return (settings) => {
    const at = settings.combos.findIndex((combo) => comboKey(combo) === comboKey(from));
    if (at < 0 || comboProblem(to, settings.combos.filter((_, index) => index !== at)) !== null) {
      return null;
    }
    return { ...settings, combos: settings.combos.map((combo, index) => (index === at ? to : combo)) };
  };
}

/** Removes the combination `gone`; doesn't apply once it is already gone. */
export function removeCombo(gone: Combo): MouseChange {
  return (settings) => {
    const combos = settings.combos.filter((combo) => comboKey(combo) !== comboKey(gone));
    return combos.length === settings.combos.length ? null : { ...settings, combos };
  };
}

/**
 * Combination changes not saved yet. They are saved one at a time (`nextComboStep`), each
 * worked out from the combinations as saved, so none depends on another still saving: edits
 * and removals by the combination they change, and how many to add.
 */
export interface ComboPlan {
  /** An edit (incomplete, clashing, or waiting its turn) or a removal, by `comboKey`. */
  readonly edits: ReadonlyMap<string, Combo | "remove">;
  readonly adds: number;
}

export const noComboPlan: ComboPlan = { edits: new Map(), adds: 0 };

/** One combination change being saved. */
export type ComboStep = { kind: "replace"; from: Combo; to: Combo } | { kind: "remove"; combo: Combo } | { kind: "add" };

const withEdit = (plan: ComboPlan, key: string, edit: Combo | "remove" | null): ComboPlan => {
  const edits = new Map(plan.edits);
  if (edit === null) {
    edits.delete(key);
  } else {
    edits.set(key, edit);
  }
  return { ...plan, edits };
};

/** `plan` with `combo` edited to `next`; an edit back to what is shown is no edit. */
export const planEdit = (plan: ComboPlan, combo: Combo, next: Combo): ComboPlan => withEdit(plan, comboKey(combo), comboKey(next) === comboKey(combo) ? null : next);
export const planRemove = (plan: ComboPlan, combo: Combo): ComboPlan => withEdit(plan, comboKey(combo), "remove");
export const planAdd = (plan: ComboPlan): ComboPlan => ({ ...plan, adds: plan.adds + 1 });

/** The rows the page shows: each combination with its edit, without those being removed. */
export function plannedRows(combos: readonly Combo[], plan: ComboPlan): { combo: Combo; draft: Combo }[] {
  return combos.flatMap((combo) => {
    const edit = plan.edits.get(comboKey(combo));
    return edit === "remove" ? [] : [{ combo, draft: edit ?? combo }];
  });
}

/**
 * The next change to save from `plan`, among `combos` as saved (call it only while no
 * combination change is saving): removals first, then additions, then the first complete edit
 * that repeats no other combination. `null` when nothing can be saved now; `plan` without what
 * the step takes (and without changes to combinations no longer listed).
 */
export function nextComboStep(combos: readonly Combo[], plan: ComboPlan): { step: ComboStep | null; plan: ComboPlan } {
  const keys = new Set(combos.map(comboKey));
  const edits = new Map([...plan.edits].filter(([key]) => keys.has(key)));
  const left = edits.size === plan.edits.size ? plan : { ...plan, edits };
  const removing = combos.find((combo) => edits.get(comboKey(combo)) === "remove");
  if (removing !== undefined) {
    return { step: { kind: "remove", combo: removing }, plan: withEdit(left, comboKey(removing), null) };
  }
  if (plan.adds > 0) {
    return { step: { kind: "add" }, plan: { ...left, adds: plan.adds - 1 } };
  }
  for (const from of combos) {
    const edit = edits.get(comboKey(from));
    if (edit !== undefined && edit !== "remove" && comboProblem(edit, combos.filter((combo) => combo !== from)) === null) {
      return { step: { kind: "replace", from, to: edit }, plan: withEdit(left, comboKey(from), null) };
    }
  }
  return { step: null, plan: left };
}

/** The settings change that saves `step`. */
export function stepChange(step: ComboStep): MouseChange {
  return step.kind === "replace" ? replaceCombo(step.from, step.to) : step.kind === "remove" ? removeCombo(step.combo) : addCombo;
}

/**
 * After `step` failed to save, what it changed is shown as saved again: an edit or removal made
 * to `to` while it was saving moves back to `from`, so it isn't lost.
 */
export function planAfterFailure(plan: ComboPlan, step: ComboStep): ComboPlan {
  if (step.kind !== "replace") {
    return plan;
  }
  const newer = plan.edits.get(comboKey(step.to));
  if (newer === undefined) {
    return plan;
  }
  const moved = withEdit(plan, comboKey(step.to), null);
  return plan.edits.has(comboKey(step.from)) ? moved : withEdit(moved, comboKey(step.from), newer);
}

/** Where the Mouse settings stand, in one line and a sentence. */
export interface MouseState {
  tone: "ok" | "warn" | "muted";
  title: string;
  detail: string;
}

export function mouseState(status: MouseStatus | null): MouseState {
  if (status === null) {
    return { tone: "muted", title: "Reading…", detail: "Checking your mouse settings." };
  }
  if (status.access === "unsupported") {
    return { tone: "muted", title: "Only on macOS", detail: "This copy of Kleari can’t change the mouse." };
  }
  if (!status.settings.enabled) {
    return { tone: "muted", title: "Off", detail: "Turn on the switch at the top to use these settings." };
  }
  if (status.access === "notAllowed") {
    return { tone: "warn", title: "Waiting for Accessibility", detail: "Nothing changes until you allow Kleari under Privacy & Security › Accessibility (below)." };
  }
  if (!status.active) {
    return {
      tone: "warn",
      title: "macOS didn’t start them",
      detail: "Turn the switch off and on again. If that doesn’t help, turn Kleari off and on in Privacy & Security › Accessibility.",
    };
  }
  return { tone: "ok", title: "On now", detail: "Your mouse settings are working." };
}

/** What would stop the settings working, as a reminder: they only work while Kleari runs. */
export function keepRunningNotes(menuBar: boolean | null, openAtLogin: LoginItem | null): string[] {
  const notes: string[] = [];
  if (menuBar === false) {
    notes.push("Closing Kleari’s window quits it, and these settings stop. Turn on Show in menu bar to keep them working with the window closed.");
  }
  if (openAtLogin !== null && openAtLogin !== "enabled" && openAtLogin !== "requiresApproval") {
    notes.push("After you restart your Mac they stay off until you open Kleari. Turn on Open at login to have them on from the start.");
  }
  return notes;
}

/** Buttons the settings use (alone or in a combination) that the button finder has never seen. */
export function unseenButtons(settings: MouseSettings, seen: readonly number[]): number[] {
  const used = new Set([...settings.buttons.map((mapping) => mapping.button), ...settings.combos.flatMap((combo) => combo.buttons)]);
  // Every mouse has left and right click.
  return [...used].filter((button) => !isClick(button) && !seen.includes(button)).sort((a, b) => a - b);
}

/** A browser key event's `code` → the key name the backend knows. */
const codeKeys: Record<string, ShortcutKey> = {
  Space: "space",
  Enter: "return",
  Tab: "tab",
  Escape: "escape",
  Backspace: "delete",
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
  BracketLeft: "[",
  BracketRight: "]",
  Minus: "-",
  Equal: "=",
};

export interface KeyPress {
  code: string;
  metaKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  ctrlKey: boolean;
}

/** The shortcut a key press makes; `null` for a modifier alone or a key Kleari can't send. */
export function shortcutFrom(press: KeyPress): Shortcut | null {
  const letter = /^Key([A-Z])$/.exec(press.code)?.[1]?.toLowerCase();
  const digit = /^Digit([0-9])$/.exec(press.code)?.[1];
  const fn = /^F([1-9]|1[0-2])$/.exec(press.code) ? press.code.toLowerCase() : undefined;
  const name = letter ?? digit ?? fn ?? codeKeys[press.code];
  const key = shortcutKeys.find((candidate) => candidate === name);
  if (key === undefined) {
    return null;
  }
  return { key, modifiers: { command: press.metaKey, shift: press.shiftKey, option: press.altKey, control: press.ctrlKey } };
}

const keyLabels: Partial<Record<ShortcutKey, string>> = {
  return: "↩",
  tab: "⇥",
  space: "Space",
  delete: "⌫",
  escape: "⎋",
  left: "←",
  right: "→",
  up: "↑",
  down: "↓",
};

/** A part of the drawn mouse that lights up. */
export type MousePart = "left" | "right" | "middle" | "back" | "forward" | "button6" | "button7" | "button8" | "wheelUp" | "wheelDown" | "wheelLeft" | "wheelRight";

const buttonParts: readonly MousePart[] = ["left", "right", "middle", "back", "forward", "button6", "button7", "button8"];

/** The drawn part for a button as macOS numbers it; `null` for one beyond button 8. */
export function partOf(button: number): MousePart | null {
  return buttonParts[button] ?? null;
}

/** A button as people name it. */
export function buttonLabel(button: number): string {
  if (button === 0) {
    return "Left click";
  }
  if (button === 1) {
    return "Right click";
  }
  return buttonNames[button]?.label ?? `Button ${button + 1}`;
}

/** How long a part stays lit after a press or a wheel step. */
export const FLASH_MS = 600;

/** What the button finder has seen while the page is open. */
export interface Finder {
  /** Buttons held down now. */
  held: number[];
  flashes: { part: MousePart; until: number }[];
  /** Every button pressed so far, in order. */
  seen: number[];
  /** What happened last, in words. */
  last: string | null;
  /** How the wheel last scrolled: in steps (a mouse) or smoothly (a trackpad or Magic Mouse). */
  wheel: "stepped" | "smooth" | null;
}

export const emptyFinder: Finder = { held: [], flashes: [], seen: [], last: null, wheel: null };

const wheelParts = (vertical: number, horizontal: number): MousePart[] => [
  ...(vertical > 0 ? (["wheelUp"] as const) : vertical < 0 ? (["wheelDown"] as const) : []),
  ...(horizontal > 0 ? (["wheelLeft"] as const) : horizontal < 0 ? (["wheelRight"] as const) : []),
];

const wheelWords: Record<string, string> = { wheelUp: "Wheel up", wheelDown: "Wheel down", wheelLeft: "Wheel left", wheelRight: "Wheel right" };

/** The finder after one report from Kleari, at `now` (ms). */
export function finderStep(finder: Finder, probe: Probe, now: number): Finder {
  const live = finder.flashes.filter((flash) => flash.until > now);
  if (probe.kind === "down") {
    const part = partOf(probe.button);
    return {
      ...finder,
      held: finder.held.includes(probe.button) ? finder.held : [...finder.held, probe.button],
      flashes: part ? [...live, { part, until: now + FLASH_MS }] : live,
      seen: finder.seen.includes(probe.button) ? finder.seen : [...finder.seen, probe.button].sort((a, b) => a - b),
      last: buttonLabel(probe.button),
    };
  }
  if (probe.kind !== "scroll") {
    return { ...finder, held: finder.held.filter((button) => button !== probe.button), flashes: live };
  }
  const parts = wheelParts(probe.vertical, probe.horizontal);
  return {
    ...finder,
    flashes: [...live, ...parts.map((part) => ({ part, until: now + FLASH_MS }))],
    last: parts.length > 0 ? parts.map((part) => wheelWords[part]).join(" and ") : finder.last,
    wheel: probe.stepped ? "stepped" : "smooth",
  };
}

/** Parts lit at `now`: held buttons and recent flashes. */
export function litParts(finder: Finder, now: number): Set<MousePart> {
  const lit = new Set<MousePart>();
  for (const button of finder.held) {
    const part = partOf(button);
    if (part) {
      lit.add(part);
    }
  }
  for (const flash of finder.flashes) {
    if (flash.until > now) {
      lit.add(flash.part);
    }
  }
  return lit;
}

/** What the wheel's way of scrolling means for the settings. */
export function wheelNote(wheel: Finder["wheel"]): string {
  switch (wheel) {
    case "stepped":
      return "Your wheel scrolls in steps, like a mouse wheel: the scrolling settings below apply to it.";
    case "smooth":
      return "That scrolled smoothly, like a trackpad or Apple’s Magic Mouse: Kleari leaves it as it is.";
    case null:
      return "Turn the wheel to see how it scrolls.";
  }
}

/** "⌃⌥⇧⌘T", in macOS's modifier order. */
export function formatShortcut(shortcut: Shortcut): string {
  const { modifiers } = shortcut;
  const keyLabel = keyLabels[shortcut.key] ?? shortcut.key.toUpperCase();
  return `${modifiers.control ? "⌃" : ""}${modifiers.option ? "⌥" : ""}${modifiers.shift ? "⇧" : ""}${modifiers.command ? "⌘" : ""}${keyLabel}`;
}
