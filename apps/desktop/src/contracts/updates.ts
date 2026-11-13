import { ContractError, record } from "./systemMetrics";

/** What a check for updates found. Versions are as released, such as `0.1.0-beta.2`. */
export type UpdateCheck =
  | { status: "upToDate"; current: string }
  | { status: "available"; current: string; version: string; notes: string | null }
  /** The channel checked (betas, or stable only) has no release yet. */
  | { status: "noRelease"; current: string; beta: boolean };

const MAX_VERSION = 64;
const MAX_NOTES = 1100;

function text(value: unknown, field: string, max: number): string {
  if (typeof value !== "string" || value.length === 0 || value.length > max) {
    throw new ContractError(field);
  }
  return value;
}

export function parseUpdateCheck(payload: unknown): UpdateCheck {
  const fields = record(payload, "$");
  const current = text(fields.current, "current", MAX_VERSION);
  if (fields.status === "upToDate") {
    return { status: "upToDate", current };
  }
  if (fields.status === "available") {
    return {
      status: "available",
      current,
      version: text(fields.version, "version", MAX_VERSION),
      notes: fields.notes === null ? null : text(fields.notes, "notes", MAX_NOTES),
    };
  }
  if (fields.status === "noRelease") {
    if (typeof fields.beta !== "boolean") {
      throw new ContractError("beta");
    }
    return { status: "noRelease", current, beta: fields.beta };
  }
  throw new ContractError("status");
}
