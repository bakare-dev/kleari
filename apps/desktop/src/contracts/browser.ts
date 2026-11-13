// IPC contract for Browser care. Mirrors src-tauri/src/browser.rs.

import { ContractError, nonNegativeInteger, oneOf, record } from "./systemMetrics";

export const browsers = ["safari", "chrome", "brave", "edge"] as const;
export type Browser = (typeof browsers)[number];

export interface BrowserTab {
  id: string;
  window: number;
  title: string;
  site: string;
}

export interface TabList {
  browser: Browser;
  status: "listed" | "notRunning" | "notAllowed" | "failed";
  tabs: BrowserTab[];
}

function text(value: unknown, field: string): string {
  if (typeof value !== "string" || value.length > 4096) {
    throw new ContractError(field);
  }
  return value;
}

export function parseTabList(payload: unknown): TabList {
  const fields = record(payload, "$");
  if (!Array.isArray(fields.tabs) || fields.tabs.length > 2000) {
    throw new ContractError("tabs");
  }
  return {
    browser: oneOf(browsers, fields.browser, "browser"),
    status: oneOf(["listed", "notRunning", "notAllowed", "failed"] as const, fields.status, "status"),
    tabs: fields.tabs.map((entry: unknown, index: number) => {
      const tab = record(entry, `tabs[${index}]`);
      const id = text(tab.id, `tabs[${index}].id`);
      if (!/^[0-9a-f]{1,64}$/.test(id)) {
        throw new ContractError(`tabs[${index}].id`);
      }
      return {
        id,
        window: nonNegativeInteger(tab.window, `tabs[${index}].window`),
        title: text(tab.title, `tabs[${index}].title`),
        site: text(tab.site, `tabs[${index}].site`),
      };
    }),
  };
}

export function parseClosed(payload: unknown): { closed: number; notAllowed: boolean } {
  const fields = record(payload, "$");
  if (typeof fields.notAllowed !== "boolean") {
    throw new ContractError("notAllowed");
  }
  return { closed: nonNegativeInteger(fields.closed, "closed"), notAllowed: fields.notAllowed };
}
