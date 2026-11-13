import type { KeyboardEvent } from "react";
import type { MousePart } from "../../../features/mouse/mouseView";

interface MouseDiagramProps {
  lit: ReadonlySet<MousePart>;
  /** Buttons pressed at least once on this page, as macOS numbers them. */
  seen: readonly number[];
  /** Jumps to a button's setting (buttons 3–8 only; left and right click aren't changed). */
  onPick: (button: number) => void;
}

/** A changeable part of the drawing, as a keyboard-reachable button. */
function Pickable({ button, label, part, lit, seen, onPick, children }: { button: number; label: string; part: MousePart; lit: ReadonlySet<MousePart>; seen: readonly number[]; onPick: (button: number) => void; children: React.ReactNode }) {
  const choose = () => onPick(button);
  return (
    <g
      role="button"
      tabIndex={0}
      aria-label={`${label}: go to its setting`}
      className={`md-part md-pick${lit.has(part) ? " lit" : ""}${seen.includes(button) ? " seen" : ""}`}
      onClick={choose}
      onKeyDown={(event: KeyboardEvent) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          choose();
        }
      }}
    >
      {children}
    </g>
  );
}

/**
 * A mouse seen from above, right-handed: left and right buttons, the wheel (with its four
 * directions), the two side buttons under the thumb and buttons 6–8 for mice that have them.
 * A part lights while it is pressed or just used.
 */
export function MouseDiagram({ lit, seen, onPick }: MouseDiagramProps) {
  const on = (part: MousePart) => (lit.has(part) ? " lit" : "");
  const found = (button: number) => (seen.includes(button) ? " seen" : "");
  return (
    <svg className="mouse-diagram" viewBox="0 0 220 280" role="group" aria-label="Your mouse, from above">
      <path className="md-body" d="M110 14C160 14 182 60 182 120V190C182 242 150 268 110 268S38 242 38 190V120C38 60 60 14 110 14Z" />
      <path className={`md-part${on("left")}${found(0)}`} d="M108 17C68 19 41 60 41 117H108Z">
        <title>Left click</title>
      </path>
      <path className={`md-part${on("right")}${found(1)}`} d="M112 17C152 19 179 60 179 117H112Z">
        <title>Right click</title>
      </path>
      <text className="md-label" x="74" y="84">L</text>
      <text className="md-label" x="140" y="84">R</text>

      <path className={`md-arrow${on("wheelUp")}`} d="M110 30l-7 9h14z" />
      <path className={`md-arrow${on("wheelDown")}`} d="M110 108l-7-9h14z" />
      <path className={`md-arrow${on("wheelLeft")}`} d="M92 69l9-7v14z" />
      <path className={`md-arrow${on("wheelRight")}`} d="M128 69l-9-7v14z" />
      <Pickable button={2} label="Middle button (pressing the wheel)" part="middle" lit={lit} seen={seen} onPick={onPick}>
        <rect x="102" y="44" width="16" height="50" rx="8" />
        <title>Middle button: press the wheel</title>
      </Pickable>

      <Pickable button={4} label="Button 5, front side button" part="forward" lit={lit} seen={seen} onPick={onPick}>
        <rect x="28" y="128" width="14" height="30" rx="6" />
        <title>Button 5 (usually Forward)</title>
      </Pickable>
      <Pickable button={3} label="Button 4, rear side button" part="back" lit={lit} seen={seen} onPick={onPick}>
        <rect x="28" y="164" width="14" height="30" rx="6" />
        <title>Button 4 (usually Back)</title>
      </Pickable>
      <text className="md-label" x="12" y="148">5</text>
      <text className="md-label" x="12" y="184">4</text>

      {([5, 6, 7] as const).map((button, index) => {
        const part = (["button6", "button7", "button8"] as const)[index];
        return (
          <Pickable key={button} button={button} label={`Button ${button + 1}`} part={part} lit={lit} seen={seen} onPick={onPick}>
            <rect x="194" y={150 + index * 30} width="22" height="22" rx="7" />
            <text className="md-chip-label" x="205" y={165 + index * 30}>{button + 1}</text>
          </Pickable>
        );
      })}
    </svg>
  );
}
