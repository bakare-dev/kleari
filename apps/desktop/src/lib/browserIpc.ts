import { parseClosed, parseTabList, type Browser, type TabList } from "../contracts/browser";
import { call } from "./storageIpc";

/** Lists a browser's tabs; macOS asks the first time whether Kleari may control it. */
export function listBrowserTabs(browser: Browser): Promise<TabList> {
  return call("list_browser_tabs", { browser }, parseTabList);
}

/** Closes the chosen tabs, each only if it still shows what it showed when listed. */
export function closeBrowserTabs(ids: readonly string[]): Promise<{ closed: number; notAllowed: boolean }> {
  return call("close_browser_tabs", { ids }, parseClosed);
}
