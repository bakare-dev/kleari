# ADR-025: A local log, and reporting a problem

- **Status:** Accepted, by the product owner's decision.
- **Context:** Problems with the Mouse module, shortcut recording and the keyboard lock (an
  event tap that stopped, system shortcuts left off, a window action an app refused) left no
  trace: Kleari kept no log, and a bug report had only the reporter's memory to go on. Any log
  must keep Kleari's privacy promise: nothing it sees (keys, files, windows, tabs) may end up
  in it.
- **Decision:**
  - **One log file, on this Mac only:** `~/Library/Logs/Kleari/kleari.log`, folder 0700 and
    file 0600, opened for appending without following a symbolic link (`log.rs`). Each line is
    `<UTC time> [<area>] <text>`, written as it happens; nothing is logged per mouse or key
    event, so writes are rare.
  - **What can be written is fixed in the code.** The API takes only `&'static str` text, with
    at most a number (an error code, a count, a length in seconds) or the name of an
    Accessibility attribute written in the code. A key, path, file name, window title or
    address can't be passed to it. Recorded: launch (with the version) and quit; the Mouse
    event tap starting (which clicks it sees), refused, stopped, turned back on after macOS's
    timeout, and Accessibility withdrawn; shortcut recording starting, ending (and whether
    the time ran out) or failing to capture the keyboard; how many system shortcuts were
    turned off, kept on, turned back on or left off for the next launch, and the restore at
    launch after a crash; the keyboard lock starting (its length), refused, and how it ended
    (click, time, stopped); window actions macOS refused (the step, the attribute and
    `AXError`); the bundle-identifier move (ADR-024) and uninstall's permission reset; every
    error a command returns to the interface, by its code's name (logged where the error is
    serialized for the page, which happens once per error returned); and panics.
  - **Panics** are the one run-time text: the source location, and the message cut to 300
    characters, on one line, with every word that contains `/` or starts with `~` replaced by
    `<path>`. macOS's own crash report (`~/Library/Logs/DiagnosticReports`) still covers
    crashes outside Rust.
  - **Size, not age, bounds it:** at 1 MB the file is renamed `kleari.1.log`, replacing the
    one before, so the log holds at most about 2 MB. Uninstall moves the folder to the Trash.
  - **Report a problem** (Settings › About › Help) opens
    `https://github.com/bakare-dev/kleari/issues/new` in the browser, the address built in Rust
    and fully percent-encoded, with a body holding the Kleari version, the macOS version
    (`kern.osproductversion`, digits and dots only), the kind of chip (`hw.optional.arm64`:
    Apple silicon or Intel) and empty "What happened", "What you expected" and "Steps to
    reproduce" sections. The log is never put in the address: Kleari shows it in Finder so the
    user can read it and attach it if they choose. "Show in Finder" shows the log on its own.
- **Consequences:** Reports can carry a log that says what Kleari did, without what the user
  had. Nothing new reaches the network: the browser opens a GitHub page and sends nothing
  until the user submits (`docs/network.md`). The Cleanup scan, which reads `~/Library/Logs`,
  may list Kleari's own log among logs to clear.
