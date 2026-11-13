import { useEffect, useState } from "react";
import { useFinderCapture } from "../../../features/mouse/useMouseFinder";
import { Button, IconTile, PageHeader, ScrollBody, Select, Switch, Toast, useToast } from "../../ui/components";
import { headingIdFor, navigate } from "../../shell/routes";
import { Group, Row } from "../settings/SettingsScreen";
import { MAX_COMBOS, mouseButtons, type MouseStatus } from "../../../contracts/mouse";
import type { LoginItem } from "../../../contracts/settings";
import type { MouseChange } from "../../../features/mouse/mouseSaves";
import type { MouseCombos } from "../../../features/mouse/useMouseCombos";
import {
  actionFor,
  actionOptions,
  buttonNames,
  choiceOf,
  comboKey,
  formatShortcut,
  keepRunningNotes,
  mouseState,
  plannedRows,
  shortcutFrom,
  speedOptions,
  unseenButtons,
  withButton,
  buttonLabel,
  wheelNote,
  type ActionChoice,
  type Finder,
  type MousePart,
} from "../../../features/mouse/mouseView";
import { ComboEditor } from "./ComboEditor";
import { MouseDiagram } from "./MouseDiagram";
import { ShortcutRecorder } from "./ShortcutRecorder";
import "./mouse.css";

/** Whether the settings are working now, and what would stop them (they work only while
 * Kleari runs). */
export function MouseStatusCard({ status, menuBar, openAtLogin, unseen }: { status: MouseStatus | null; menuBar: boolean | null; openAtLogin: LoginItem | null; unseen: readonly number[] }) {
  const state = mouseState(status);
  const notes = status?.settings.enabled ? keepRunningNotes(menuBar, openAtLogin) : [];
  return (
    <section aria-labelledby="mouse-status-title" className="v2-card mouse-status">
      <div className="mouse-status-head">
        <span className={`v2-pill v2-pill-${state.tone === "ok" ? "ok" : state.tone === "warn" ? "warn" : "muted"}`}>{state.title}</span>
        <h2 id="mouse-status-title" className="v2-visually-hidden">Status</h2>
        <span className="v2-muted">{state.detail}</span>
      </div>
      {notes.length > 0 && (
        <div className="mouse-status-notes">
          {notes.map((note) => (
            <span key={note} className="set-row-sub">{note}</span>
          ))}
          <Button variant="soft" size="sm" onClick={() => navigate("settings", "general")}>Open Settings</Button>
        </div>
      )}
      {status?.settings.enabled && unseen.length > 0 && (
        <span className="set-row-sub">
          {unseen.map((button) => buttonNames[button].label).join(", ")} {unseen.length === 1 ? "is set up but hasn’t" : "are set up but haven’t"} lit up in Find your buttons. If
          your mouse doesn’t have {unseen.length === 1 ? "it" : "them"}, those settings never happen.
        </span>
      )}
    </section>
  );
}

/** Brings a button's setting into view and focuses it. */
function goToButton(button: number) {
  const control = document.getElementById(`mouse-button-${button}`);
  control?.scrollIntoView({ block: "center", behavior: "smooth" });
  control?.focus({ preventScroll: true });
}

/** The drawn mouse lights up as you press its buttons or turn its wheel, so you can tell which
 * is which before setting them up. */
export function ButtonFinder({ finder, lit, unavailable }: { finder: Finder; lit: ReadonlySet<MousePart>; unavailable: boolean }) {
  // The pointer is over this box: buttons and the wheel only light up the drawing here.
  const [over, setOver] = useState(false);
  useFinderCapture(over && !unavailable);
  useEffect(() => {
    // Leaving the window (for another app) ends testing too.
    const leave = () => setOver(false);
    window.addEventListener("blur", leave);
    return () => window.removeEventListener("blur", leave);
  }, []);
  return (
    <section
      aria-labelledby="mouse-finder-title"
      className={`v2-card mouse-finder${over && !unavailable ? " testing" : ""}`}
      onPointerEnter={() => setOver(true)}
      onPointerLeave={() => setOver(false)}
      // A right click here is a button being tried, not a request for a menu.
      onContextMenu={(event) => event.preventDefault()}
    >
      <MouseDiagram lit={lit} seen={finder.seen} onPick={goToButton} />
      <div className="mouse-finder-text">
        <h2 id="mouse-finder-title" className="v2-card-title">
          Find your buttons
          {over && !unavailable && <span className="v2-pill v2-pill-link mouse-finder-badge">Testing</span>}
        </h2>
        {unavailable ? (
          <span className="v2-muted">The button finder works in the Kleari app.</span>
        ) : (
          <>
            <span className="v2-muted">
              Point at this box, then press any button or turn the wheel: the matching part lights up. Here, buttons and the wheel do nothing else, and your mouse settings
              pause so you see the real buttons. Left click still works: click a part to go to its setting.
            </span>
            <span role="status" aria-live="polite" className="mouse-finder-last">{finder.last ?? "Waiting for a press or scroll…"}</span>
            <span className="v2-muted">{wheelNote(finder.wheel)}</span>
            {finder.seen.length > 0 && (
              <span className="mouse-finder-seen" aria-label="Buttons found">
                {finder.seen.map((button) => (
                  <span key={button} className="v2-pill v2-pill-link">{buttonLabel(button)}</span>
                ))}
              </span>
            )}
          </>
        )}
      </div>
    </section>
  );
}

interface MouseScreenProps {
  /** `null` while the first read is under way. */
  status: MouseStatus | null;
  /** What the button finder has seen, the parts lit now, and whether it can't run here. */
  finder: Finder;
  lit: ReadonlySet<MousePart>;
  finderUnavailable: boolean;
  /** Why the settings couldn't be read; the last read values stay shown. */
  error: string | null;
  /** Resolves with what to tell the user, or `null` once saved. */
  onSave: (change: MouseChange) => Promise<string | null>;
  /** Combination changes waiting to save, kept by the app (`useMouseCombos`). */
  combos: MouseCombos;
  onRequestAccess: () => Promise<string | null>;
  onOpenAccessibility: () => void;
  /** Settings › Show in menu bar and Open at login, which keep Kleari (and so these settings)
   * running; `null` while not read. */
  menuBar: boolean | null;
  openAtLogin: LoginItem | null;
}

export function MouseScreen({ status, finder, lit, finderUnavailable, error, onSave, combos, onRequestAccess, onOpenAccessibility, menuBar, openAtLogin }: MouseScreenProps) {
  const toast = useToast();
  // A button whose new shortcut is being recorded; nothing is saved until a key is pressed.
  const [recordingFor, setRecordingFor] = useState<number | null>(null);
  // The combination (by `comboKey`) whose press is waiting for a recorded shortcut.
  const [recordingCombo, setRecordingCombo] = useState<string | null>(null);
  const settings = status?.settings;
  const change = (update: MouseChange) => {
    void onSave(update).then((message) => message && toast.show(message, "warn"));
  };
  const disabled = !settings;
  // Combination changes not saved yet, by the combination they change (never by row, so rows
  // coming and going can't hand one to another combination), kept by the app.
  const { plan } = combos;
  const rows = plannedRows(settings?.combos ?? [], plan);
  const canAdd = settings !== undefined && rows.length + plan.adds < MAX_COMBOS;
  // A combination change that couldn't be saved, here or since leaving the page.
  const { failure, takeFailure } = combos;
  useEffect(() => {
    if (failure === null) {
      return;
    }
    const message = takeFailure();
    if (message !== null) {
      toast.show(message, "warn");
    }
  }, [failure, takeFailure, toast.show]);

  const chooseAction = (button: number, choice: ActionChoice) => {
    if (!settings) {
      return;
    }
    setRecordingFor(null);
    if (choice === "shortcut") {
      setRecordingFor(button);
      return;
    }
    change((saved) => withButton(saved, button, choice === "usual" ? null : { kind: choice }));
  };

  return (
    <>
      <PageHeader
        title="Mouse"
        headingId={headingIdFor("mouse")}
        subtitle="Make a mouse work your way: scrolling, buttons and shortcuts. Your trackpad stays as it is."
        actions={
          <Switch
            label="Use these mouse settings"
            checked={settings?.enabled ?? false}
            disabled={disabled || status?.access === "unsupported"}
            onChange={(enabled) => change((saved) => ({ ...saved, enabled }))}
          />
        }
      />
      <ScrollBody label="Mouse" className="mouse-body">
        {error && (
          <div role="alert" className="v2-banner v2-banner-warn">
            <IconTile tone="yellow" icon="warning" size={32} />
            <div className="v2-banner-text">
              <strong>The Mouse settings couldn’t be read</strong>
              <span>{error}</span>
            </div>
          </div>
        )}
        {status?.access === "notAllowed" && (
          <div role="status" className="v2-banner v2-banner-warn">
            <IconTile tone="orange" icon="lock" size={32} />
            <div className="v2-banner-text">
              <strong>Allow Kleari under Accessibility</strong>
              <span>
                macOS asks before an app can change how your mouse scrolls and what its buttons do. Once allowed, the mouse settings see only the wheel and extra buttons (and left or
                right clicks only while a combination uses one), never your keystrokes or where the pointer goes, and keep none of them. (The button finder below sees only clicks and scrolls inside Kleari’s own
                window, while this page is open.) Turn Kleari on in Privacy &amp; Security › Accessibility; this page notices by itself.
              </span>
            </div>
            <Button variant="soft" size="sm" onClick={() => void onRequestAccess().then((message) => message && toast.show(message, "warn"))}>Allow…</Button>
            <Button variant="soft" size="sm" onClick={onOpenAccessibility}>Open System Settings</Button>
          </div>
        )}
        <MouseStatusCard status={status} menuBar={menuBar} openAtLogin={openAtLogin} unseen={settings ? unseenButtons(settings, finder.seen) : []} />

        <ButtonFinder finder={finder} lit={lit} unavailable={finderUnavailable} />

        <Group
          title="Scrolling"
          note={
            status?.active
              ? "On now. They work while Kleari is open or in the menu bar."
              : "They work while Kleari is open or in the menu bar, once turned on at the top."
          }
        >
          <Row
            label="Reverse scrolling"
            sub="A mouse wheel scrolls the classic way. The trackpad and Apple’s Magic Mouse keep natural scrolling."
            control={<Switch label="Reverse scrolling" checked={settings?.reverseVertical ?? true} disabled={disabled} onChange={(on) => change((saved) => ({ ...saved, reverseVertical: on }))} />}
          />
          <Row
            label="Reverse sideways scrolling"
            sub="For a wheel that tilts or scrolls sideways"
            control={<Switch label="Reverse sideways scrolling" checked={settings?.reverseHorizontal ?? false} disabled={disabled} onChange={(on) => change((saved) => ({ ...saved, reverseHorizontal: on }))} />}
          />
          <Row
            label="Scroll speed"
            sub="How far each wheel step goes"
            control={<Select label="Scroll speed" value={String(settings?.speed ?? 100)} options={speedOptions} disabled={disabled} onChange={(speed) => change((saved) => ({ ...saved, speed: Number(speed) }))} />}
          />
          <Row
            label="Smooth scrolling"
            sub="Each wheel step glides instead of jumping"
            control={<Switch label="Smooth scrolling" checked={settings?.smooth ?? false} disabled={disabled} onChange={(on) => change((saved) => ({ ...saved, smooth: on }))} />}
          />
        </Group>

        <Group title="Buttons" note="Left and right click aren’t set up here; they can be part of a combination below. Buttons your mouse doesn’t have are simply never used.">
          {mouseButtons.map((button) => {
            const action = settings ? actionFor(settings, button) : null;
            const recording = recordingFor === button;
            const shortcut = action?.kind === "shortcut" ? formatShortcut(action.shortcut) : null;
            return (
              <Row
                key={button}
                label={buttonNames[button].label}
                sub={buttonNames[button].sub}
                control={
                  <span className="mouse-controls">
                    {(recording || shortcut !== null) && (
                      <ShortcutRecorder
                        key={recording ? "new" : shortcut}
                        current={recording ? null : shortcut}
                        onCancel={() => setRecordingFor(null)}
                        onRecord={(event) => {
                          const recorded = shortcutFrom(event);
                          if (recorded === null || !settings) {
                            return;
                          }
                          setRecordingFor(null);
                          change((saved) => withButton(saved, button, { kind: "shortcut", shortcut: recorded }));
                        }}
                      />
                    )}
                    <Select<ActionChoice>
                      id={`mouse-button-${button}`}
                      label={`${buttonNames[button].label} does`}
                      value={recording ? "shortcut" : choiceOf(action)}
                      options={actionOptions}
                      disabled={disabled}
                      onChange={(choice) => chooseAction(button, choice)}
                    />
                  </span>
                }
              />
            );
          })}
        </Group>

        <Group
          title="Combinations"
          note="Up to two buttons and any keys together, or left or right click with another button. While scrolling: hold them and turn the wheel. When pressed: press them together (the last one completes it; hold left or right click first). A button in a combination still does its own click if you let go without completing one. Left click always reaches the app as usual; right click, once part of a combination, opens its menu when you let go. Only a mouse wheel is changed, never the trackpad."
        >
          {rows.map(({ combo, draft }, index) => (
            // Keyed by place, so focus stays put while a row's edits save; everything a row shows
            // comes from its combination and that combination's planned change, never the row.
            <ComboEditor
              key={index}
              index={index}
              draft={draft}
              others={rows.filter((_, at) => at !== index).map((row) => row.combo)}
              seen={finder.seen}
              disabled={disabled}
              recording={recordingCombo === comboKey(combo)}
              onRecording={(on) => setRecordingCombo(on ? comboKey(combo) : null)}
              onEdit={(next) => combos.edit(combo, next)}
              onRemove={() => {
                // A recording for it ends with it, so it can't return on a combination added later.
                if (recordingCombo === comboKey(combo)) {
                  setRecordingCombo(null);
                }
                combos.remove(combo);
              }}
            />
          ))}
          <Row
            label="Add a combination"
            sub={
              settings && !canAdd
                ? `That’s the most there can be (${MAX_COMBOS}). Remove one to add another.`
                : "For example, hold buttons 4 and 5 and scroll to zoom, or press ⌘ and the middle button to close a tab"
            }
            control={
              <Button variant="soft" size="sm" disabled={!canAdd} onClick={combos.add}>
                Add
              </Button>
            }
          />
        </Group>
        <div className="set-end" />
      </ScrollBody>
      <Toast message={toast.message} />
    </>
  );
}
