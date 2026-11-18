import type { LegalDocument } from "../features/legal/legal";
import { ContractError } from "../contracts/systemMetrics";
import { call } from "./storageIpc";

/** Opens one of Kleari's legal documents, as this version has it, in the browser; Rust builds
 * the address from the document's name. */
export async function openLegalDocument(document: LegalDocument): Promise<void> {
  await call("open_legal_document", { document }, () => undefined);
}

/** Opens a new, pre-filled GitHub issue in the browser (Rust builds the address) and shows
 * Kleari's log in Finder; resolves to whether the log could be shown. */
export function reportProblem(): Promise<boolean> {
  return call("report_problem", {}, (payload) => {
    if (typeof payload !== "boolean") {
      throw new ContractError("$");
    }
    return payload;
  });
}

/** Shows Kleari's log in Finder. */
export async function showLog(): Promise<void> {
  await call("show_log", {}, () => undefined);
}
