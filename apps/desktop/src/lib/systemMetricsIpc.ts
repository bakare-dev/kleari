import { invoke, isTauri } from "@tauri-apps/api/core";
import { ContractError, parseSystemMetrics, type SystemMetrics } from "../contracts/systemMetrics";

export type MetricsErrorKind =
  | "backendUnavailable"
  | "metricsUnavailable"
  | "invalidResponse"
  | "unknown";

export class MetricsError extends Error {
  readonly kind: MetricsErrorKind;

  constructor(kind: MetricsErrorKind, options?: { cause?: unknown }) {
    super(`System metrics request failed: ${kind}`, options);
    this.name = "MetricsError";
    this.kind = kind;
  }
}

function isMetricsUnavailableError(error: unknown): boolean {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    error.code === "metricsUnavailable"
  );
}

/**
 * Invokes a read-only system command and validates its response, mapping failures to
 * `MetricsErrorKind`. Shared by the Overview and the System page.
 */
export async function invokeValidated<T>(command: string, parse: (payload: unknown) => T): Promise<T> {
  if (!isTauri()) {
    throw new MetricsError("backendUnavailable");
  }
  let payload: unknown;
  try {
    payload = await invoke<unknown>(command);
  } catch (error) {
    throw new MetricsError(isMetricsUnavailableError(error) ? "metricsUnavailable" : "unknown", {
      cause: error,
    });
  }
  try {
    return parse(payload);
  } catch (error) {
    if (error instanceof ContractError) {
      throw new MetricsError("invalidResponse", { cause: error });
    }
    throw error;
  }
}

/** Invokes the read-only `get_system_metrics` command and validates the response. */
export function fetchSystemMetrics(): Promise<SystemMetrics> {
  return invokeValidated("get_system_metrics", parseSystemMetrics);
}
