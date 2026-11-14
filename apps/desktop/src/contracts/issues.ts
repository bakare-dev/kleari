// IPC contract for issue alerts. Mirrors src-tauri/src/issues.rs.

import { ContractError, nonNegativeInteger, nullable, oneOf, record } from "./systemMetrics";

export const issueKinds = [
  "lowDiskSpace",
  "criticalDiskSpace",
  "lowDriveSpace",
  "heavyMemory",
  "criticalMemory",
  "busyCpu",
  "batteryDrain",
  "batteryLow",
  "batteryCritical",
  "batteryService",
  "batteryCycles",
  "batteryHot",
  "lowDeviceBattery",
  "fullTrash",
  "unusedApps",
  "manyLoginItems",
  "cleanupDue",
] as const;
export type IssueKind = (typeof issueKinds)[number];

export interface Issue {
  kind: IssueKind;
  severity: "warning" | "info";
  subject: string | null;
  /** Bytes free or held, a percent, °C, a count or days, as the kind says. */
  value: number | null;
  /** Total bytes, watts, cores or design cycles. */
  total: number | null;
}

export function parseIssues(payload: unknown): Issue[] {
  if (!Array.isArray(payload) || payload.length > 200) {
    throw new ContractError("$");
  }
  return payload.map((entry, index) => {
    const field = `[${index}]`;
    const issue = record(entry, field);
    return {
      kind: oneOf(issueKinds, issue.kind, `${field}.kind`),
      severity: oneOf(["warning", "info"] as const, issue.severity, `${field}.severity`),
      subject: nullable(issue.subject, (subject) => {
        if (typeof subject !== "string" || subject.length > 4096) {
          throw new ContractError(`${field}.subject`);
        }
        return subject;
      }),
      value: nullable(issue.value, (value) => nonNegativeInteger(value, `${field}.value`)),
      total: nullable(issue.total, (total) => nonNegativeInteger(total, `${field}.total`)),
    };
  });
}
