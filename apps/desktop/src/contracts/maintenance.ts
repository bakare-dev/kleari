// IPC contract for the Maintenance page. Mirrors src-tauri/src/maintenance.rs.

import { ContractError, nonNegativeInteger, nullable, oneOf, record } from "./systemMetrics";

export const tasks = ["freeRam", "flushDns", "thinSnapshots", "reindexSpotlight", "clearSystemCaches", "clearSystemLogs", "removeDeletedUsers", "emptyTrash"] as const;
export type Task = (typeof tasks)[number];

export interface TaskInfo {
  task: Task;
  bytes: number | null;
  lowerBound: boolean;
  count: number | null;
  deletes: boolean;
  needsAdmin: boolean;
}

export type TaskOutcome = "done" | "cancelled" | "failed";

function boolean(value: unknown, field: string): boolean {
  if (typeof value !== "boolean") {
    throw new ContractError(field);
  }
  return value;
}

export function parseTaskInfo(payload: unknown): TaskInfo[] {
  if (!Array.isArray(payload) || payload.length > tasks.length) {
    throw new ContractError("$");
  }
  return payload.map((entry, index) => {
    const field = `[${index}]`;
    const info = record(entry, field);
    return {
      task: oneOf(tasks, info.task, `${field}.task`),
      bytes: nullable(info.bytes, (bytes) => nonNegativeInteger(bytes, `${field}.bytes`)),
      lowerBound: boolean(info.lowerBound, `${field}.lowerBound`),
      count: nullable(info.count, (count) => nonNegativeInteger(count, `${field}.count`)),
      deletes: boolean(info.deletes, `${field}.deletes`),
      needsAdmin: boolean(info.needsAdmin, `${field}.needsAdmin`),
    };
  });
}

export function parseTaskOutcome(payload: unknown): TaskOutcome {
  return oneOf(["done", "cancelled", "failed"] as const, payload, "$");
}

export function parseSentAgent(payload: unknown): boolean {
  return boolean(payload, "$");
}
