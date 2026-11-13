import { useCallback, useEffect, useRef, useState } from "react";
import type { Combo } from "../../contracts/mouse";
import type { MouseChange } from "./mouseSaves";
import { nextComboStep, noComboPlan, planAdd, planAfterFailure, planEdit, planRemove, stepChange, type ComboPlan } from "./mouseView";

/**
 * Combination changes not saved yet, and their saving: one step at a time (`nextComboStep`),
 * worked out from the combinations as saved once the one before has finished, so none is built
 * on a change that then fails. Kept by the app, so changes waiting their turn still save after
 * leaving the Mouse page; a failure meanwhile is kept until the page shows it.
 *
 * `combos` is what the page shows as saved (`null` while not read); while no step is saving it
 * is the saved combinations, since only these steps change them.
 */
export function useMouseCombos(combos: readonly Combo[] | null, save: (change: MouseChange) => Promise<string | null>) {
  const [plan, setPlan] = useState<ComboPlan>(noComboPlan);
  const [saving, setSaving] = useState(false);
  // Failures not shown yet (several can happen while away). The ref hands them out once
  // (`takeFailure`), even if the page's effect runs twice; the state re-renders the page.
  const [failure, setFailure] = useState<string | null>(null);
  const unshown = useRef<string[]>([]);

  useEffect(() => {
    if (saving || combos === null) {
      return;
    }
    const next = nextComboStep(combos, plan);
    if (next.plan !== plan) {
      setPlan(next.plan);
    }
    const { step } = next;
    if (step === null) {
      return;
    }
    setSaving(true);
    // `save` never rejects: it resolves with what to tell the user.
    void save(stepChange(step)).then((message) => {
      if (message) {
        unshown.current = [...unshown.current, message];
        setFailure(message);
        setPlan((current) => planAfterFailure(current, step));
      }
      setSaving(false);
    });
  }, [saving, combos, plan, save]);

  const edit = useCallback((combo: Combo, next: Combo) => setPlan((current) => planEdit(current, combo, next)), []);
  const remove = useCallback((combo: Combo) => setPlan((current) => planRemove(current, combo)), []);
  const add = useCallback(() => setPlan(planAdd), []);
  /** The failures not shown yet, once, as one message: later calls get `null` until another. */
  const takeFailure = useCallback(() => {
    const message = failureMessage(unshown.current);
    unshown.current = [];
    setFailure(null);
    return message;
  }, []);
  return { plan, edit, remove, add, failure, takeFailure };
}

export type MouseCombos = ReturnType<typeof useMouseCombos>;

/** Combination changes that couldn't be saved, as one message: each different reason once. */
export function failureMessage(failures: readonly string[]): string | null {
  const reasons = [...new Set(failures)];
  if (reasons.length <= 1) {
    return reasons[0] ?? null;
  }
  return `${failures.length} combination changes weren’t saved. ${reasons.join(" ")}`;
}
