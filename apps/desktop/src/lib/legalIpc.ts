import type { LegalDocument } from "../features/legal/legal";
import { call } from "./storageIpc";

/** Opens one of Kleari's legal documents, as this version has it, in the browser; Rust builds
 * the address from the document's name. */
export async function openLegalDocument(document: LegalDocument): Promise<void> {
  await call("open_legal_document", { document }, () => undefined);
}
