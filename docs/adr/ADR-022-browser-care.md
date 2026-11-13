# ADR-022: Browser care

- **Status:** Accepted, by the product owner's decision (usage without permission, tab actions
  behind macOS's Automation permission).
- **Context:** People asked for a module to manage browser tabs and resource use.
- **Decision:**
  - **Usage** of Safari, Google Chrome, Brave and Microsoft Edge comes from the process list
    Kleari already reads (CPU, memory, process count), with no permission. Safari's page
    processes belong to macOS's WebKit and aren't counted; the page says so.
  - **Tabs** are listed with each browser's own AppleScript, run by `/usr/bin/osascript` when
    you choose Show tabs. macOS asks once per browser whether Kleari may control it
    (`NSAppleEventsUsageDescription`). Each script is fixed per browser; it first checks the
    browser is running, so it never opens one. Tab addresses stay in Rust; the page gets each
    tab's title and site name.
  - **Closing** a tab passes its window, place and address to a fixed script as arguments
    (never written into it); the tab closes only if it still shows that address, and later
    tabs in a window close first so earlier ones keep their places. The page asks first.
  - Tabs can't be put to sleep from outside a browser; the page points to each browser's own
    memory saver, Cleanup for caches and Processes to quit.
- **Consequences:** Kleari can close browser tabs you choose. Nothing is stored or sent; tab
  titles are shown on screen only.
