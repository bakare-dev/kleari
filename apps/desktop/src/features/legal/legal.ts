/** Kleari's legal documents and the "use at your own risk" notice (DISCLAIMER.md). */

/** The notice's version: raising it shows the notice again to everyone, once. */
export const DISCLAIMER_VERSION = 1;

/** The notice in one sentence, for the sidebar's ⓘ and Settings › About. */
export const DISCLAIMER_SHORT =
  "Kleari is free and open source, and provided as is. Nothing changes without your OK, and anything it moves can be put back from quarantine.";

/** The documents Settings › About opens (src-tauri `LegalDocument`). */
export const legalDocuments = [
  { id: "disclaimer", label: "Disclaimer", sub: "Provided as is: what to keep in mind" },
  { id: "privacy", label: "Privacy", sub: "What Kleari reads, and that it stays on your Mac" },
  { id: "license", label: "Licence", sub: "GNU General Public License v3.0 or later: use, change and share it" },
  { id: "thirdPartyNotices", label: "Third-party notices", sub: "The open-source software inside Kleari, and its licences" },
  { id: "security", label: "Security", sub: "How to report a vulnerability privately" },
] as const;

export type LegalDocument = (typeof legalDocuments)[number]["id"];

export const COPYRIGHT = "© 2026 the Kleari contributors";
