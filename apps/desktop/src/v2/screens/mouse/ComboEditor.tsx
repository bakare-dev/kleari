import { Button, Segmented, Select } from "../../ui/components";
import { isClick, type Combo, type ComboAction, type ScrollAction } from "../../../contracts/mouse";
import {
  buttonNames,
  comboButtonOptions,
  comboButtons,
  comboName,
  comboProblem,
  formatShortcut,
  modifierKeys,
  pressActionOptions,
  scrollActionOptions,
  shortcutFrom,
  type ActionChoice,
} from "../../../features/mouse/mouseView";
import { ShortcutRecorder } from "./ShortcutRecorder";

interface ComboEditorProps {
  index: number;
  /** The combination as shown: its edit not saved yet, or what is saved (or saving). */
  draft: Combo;
  /** The other combinations, which this one must not repeat. */
  others: readonly Combo[];
  /** Buttons the button finder has seen; a combination using another is flagged. */
  seen: readonly number[];
  disabled: boolean;
  /** Whether a keyboard shortcut is being recorded for its press (kept by the page, with the
   * combination, so it never moves to another row). */
  recording: boolean;
  onRecording: (on: boolean) => void;
  /** Every edit; the page saves it once it is complete. */
  onEdit: (next: Combo) => void;
  onRemove: () => void;
}

/**
 * One combination: up to two buttons and any keys, held while the wheel turns or pressed
 * together. The page saves an edit as soon as it is complete; until then the editor says
 * what is missing.
 */
export function ComboEditor({ index, draft, others, seen, disabled, recording, onRecording: setRecording, onEdit, onRemove }: ComboEditorProps) {
  const problem = comboProblem(draft, others);
  const update = onEdit;
  const label = `Combination ${index + 1}`;
  const [first = "none", second = "none"] = draft.buttons.map(String);
  const setButtons = (slotA: string, slotB: string) => update({ ...draft, buttons: comboButtons(slotA, slotB) });
  // Every mouse has left and right click.
  const unseen = draft.buttons.filter((button) => !isClick(button) && !seen.includes(button));
  const pressAction = draft.does.kind === "press" ? draft.does.action : null;
  const shortcut = pressAction?.kind === "shortcut" ? formatShortcut(pressAction.shortcut) : null;
  const setDoes = (does: ComboAction) => update({ ...draft, does });

  return (
    <div className="mouse-combo" role="group" aria-label={label}>
      <div className="mouse-combo-head">
        <span className="v2-row-text">
          <span className="set-row-label">{comboName(draft)}</span>
          <span className="set-row-sub">{label}</span>
        </span>
        <Button variant="soft" size="sm" disabled={disabled} onClick={onRemove}>Remove</Button>
      </div>
      <div className="mouse-combo-fields">
        <span className="mouse-combo-label">Buttons</span>
        <span className="mouse-controls">
          <Select label={`${label}: first button`} value={first} options={comboButtonOptions} disabled={disabled} onChange={(value) => setButtons(value, second)} />
          <span aria-hidden="true">+</span>
          <Select label={`${label}: second button`} value={second} options={comboButtonOptions} disabled={disabled} onChange={(value) => setButtons(first, value)} />
        </span>
        <span className="mouse-combo-label">Keys</span>
        <span className="mouse-controls mouse-keys">
          {modifierKeys.map(({ key, symbol, name }) => (
            <button
              key={key}
              type="button"
              className={`mouse-key${draft.modifiers[key] ? " on" : ""}`}
              aria-pressed={draft.modifiers[key]}
              aria-label={`${label}: hold ${name}`}
              disabled={disabled}
              onClick={() => update({ ...draft, modifiers: { ...draft.modifiers, [key]: !draft.modifiers[key] } })}
            >
              {symbol}
            </button>
          ))}
        </span>
        <span className="mouse-combo-label">Does</span>
        <span className="mouse-controls">
          <Segmented<"wheel" | "press">
            label={`${label}: when`}
            kind="radio"
            value={draft.does.kind}
            onChange={(kind) => setDoes(kind === "wheel" ? { kind: "wheel", wheel: "switchSpaces" } : { kind: "press", action: { kind: "missionControl" } })}
            options={[
              { value: "wheel", label: "While scrolling" },
              { value: "press", label: "When pressed" },
            ]}
          />
          {draft.does.kind === "wheel" ? (
            <Select<ScrollAction>
              label={`${label}: the wheel`}
              value={draft.does.wheel}
              options={scrollActionOptions}
              disabled={disabled}
              onChange={(wheel) => setDoes({ kind: "wheel", wheel })}
            />
          ) : (
            <>
              {(recording || shortcut !== null) && (
                <ShortcutRecorder
                  key={recording ? "new" : shortcut}
                  current={recording ? null : shortcut}
                  onCancel={() => setRecording(false)}
                  onRecord={(event) => {
                    const recorded = shortcutFrom(event);
                    if (recorded !== null) {
                      setRecording(false);
                      setDoes({ kind: "press", action: { kind: "shortcut", shortcut: recorded } });
                    }
                  }}
                />
              )}
              <Select<ActionChoice>
                label={`${label}: pressed, does`}
                value={recording ? "shortcut" : (pressAction?.kind ?? "missionControl")}
                options={pressActionOptions}
                disabled={disabled}
                onChange={(choice) => {
                  if (choice === "shortcut") {
                    setRecording(true);
                  } else if (choice !== "usual") {
                    setRecording(false);
                    setDoes({ kind: "press", action: { kind: choice } });
                  }
                }}
              />
            </>
          )}
        </span>
      </div>
      {problem && (
        <p role="status" className="set-row-sub mouse-combo-note">
          Not saved yet: {problem}
        </p>
      )}
      {!problem && unseen.length > 0 && (
        <p className="set-row-sub mouse-combo-note">
          {unseen.map((button) => buttonNames[button].label).join(" and ")} {unseen.length === 1 ? "hasn’t" : "haven’t"} lit up in Find your buttons yet. If your mouse doesn’t
          have {unseen.length === 1 ? "it" : "them"}, this combination never happens.
        </p>
      )}
    </div>
  );
}
