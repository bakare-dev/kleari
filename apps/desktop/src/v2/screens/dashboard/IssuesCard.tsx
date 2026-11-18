import type { Issue } from "../../../contracts/issues";
import { issueLook, issueText, sortedIssues } from "../../../features/issues/issueView";
import { IconTile, Pill } from "../../ui/components";
import { hrefFor } from "../../shell/routes";

/** Issue alerts on the Dashboard: what is worth attention now, each with where to deal with it. */
export function IssuesCard({ issues, failed }: { issues: Issue[] | null; failed: boolean }) {
  if (failed || !issues || issues.length === 0) {
    return null;
  }
  return (
    <section aria-labelledby="dash-issues" className="v2-card dash-issues">
      <h2 id="dash-issues" className="v2-card-title-sm">Worth your attention</h2>
      <ul className="dash-issue-rows">
        {sortedIssues(issues).map((issue, index) => {
          const meta = issueLook[issue.kind];
          const { title, sub } = issueText(issue);
          return (
            <li key={`${issue.kind}-${issue.subject ?? index}`}>
              <IconTile tone={meta.tone} icon={meta.icon} size={30} />
              <span className="v2-row-text">
                <span className="v2-row-title">{title}</span>
                <span className="v2-row-sub v2-ellipsis">{sub}</span>
              </span>
              {issue.severity === "warning" && <Pill status="warn">Warning</Pill>}
              <a className="v2-link" href={hrefFor(meta.route)}>{meta.action}</a>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
