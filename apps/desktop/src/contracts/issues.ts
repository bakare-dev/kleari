// IPC contract for issue alerts. Mirrors src-tauri/src/issues.rs.

import { ContractError, nonNegativeInteger, nullable, oneOf, record } from "./systemMetrics";

export type IssueKind = "lowDiskSpace" | "lowDriveSpace" | "heavyMemory" | "batteryDrain" | "lowDeviceBattery" | "fullTrash" | "unusedApps";

export interface Issue {
  kind: IssueKind;
  severity: "warning" | "info";
  subject: string | null;
  value: number | null;
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
      kind: oneOf(["lowDiskSpace", "lowDriveSpace", "heavyMemory", "batteryDrain", "lowDeviceBattery", "fullTrash", "unusedApps"] as const, issue.kind, `${field}.kind`),
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
