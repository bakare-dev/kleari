import { StatusMark } from "../../ui/icons";
import { hrefFor, type V2Route } from "../../shell/routes";

const steps: readonly { route: V2Route; label: string }[] = [
  { route: "cleanup", label: "Scan" },
  { route: "review", label: "Review results" },
  { route: "preview", label: "Preview and approve" },
];

/**
 * The three Cleanup steps. Earlier steps link back; later steps are not links, because the
 * only way forward is through the current step (results first, then an explicit preview).
 */
export function CleanupSteps({ current }: { current: 0 | 1 | 2 }) {
  return (
    <nav aria-label="Cleanup steps" className="clean-steps">
      {steps.map((step, index) => {
        const label = `${index + 1} · ${step.label}`;
        if (index < current) {
          return (
            <a key={step.route} className="clean-step done" href={hrefFor(step.route)}>
              <StatusMark tone="ok" size={14} />
              {label}
            </a>
          );
        }
        if (index === current) {
          return (
            <span key={step.route} className="clean-step current" aria-current="step">
              <span className="clean-step-num" aria-hidden="true">{index + 1}</span>
              {step.label}
            </span>
          );
        }
        return (
          <span key={step.route} className="clean-step upcoming" aria-disabled="true">
            <span className="clean-step-num" aria-hidden="true">{index + 1}</span>
            {step.label}
          </span>
        );
      })}
    </nav>
  );
}
