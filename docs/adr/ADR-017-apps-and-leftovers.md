# ADR-017: Apps page, app removal and leftovers

- **Status:** Accepted, by the product owner's decision. It amends ADR-012: Application
  Support and the app containers, until now never touched, can be reached for one kind of
  item (an app's own leftover), and Kleari can move an app bundle to the Trash.
- **Context:** Removing an app on macOS leaves its settings, caches and data in the user's
  Library, and its installer in Downloads. People asked for an uninstaller that removes an
  app completely, a way to clear what removed apps left behind, and old installers. ADR-012
  kept `~/Library/Application Support`, `Containers` and `Group Containers` out of reach
  entirely, because they hold apps' real data.
- **Decision:**
  - **A read-only Apps scan** (`apps/`). It reads the app bundles directly in
    `/Applications`, `/Applications/Utilities` and `~/Applications` (each one's `Info.plist`
    only, at most 256 KB, as untrusted input), and the names and sizes of entries directly in
    the Library folders apps keep data in (`policy::LEFTOVER_FOLDERS`). Everything is opened
    through handles without following symlinks; a symlinked app (Safari) isn't offered.
  - **Matching.** An entry belongs to an app when its name is the app's bundle ID or name
    (ignoring case and a `.plist`, `.savedState` or `.binarycookies` suffix), that name followed
    by a dot (`ByHost/<id>.<host>.plist`, `<id>.helper.plist`), or a group container
    `<team>.<id>` (`policy::leftover_matches`). Apple's (`com.apple.…`) never match.
  - **Removing an app** moves its bundle to the Trash through macOS (`NSFileManager
    trashItemAtURL`), so Finder's Put Back works and macOS asks for App Management permission
    when it needs to. It is refused for an app that is part of macOS, not directly in an
    Applications folder, open now (the running apps are read fresh; without them nothing is
    removed), or no longer the folder the scan saw (device and inode). The page asks first and
    says what happens. Activity records it (`appRemoved`).
  - **Its leftovers** are then read again and sent to Cleanup's review, marked as the removed
    app's own (Low risk). They move only when the user selects them and approves a preview,
    into quarantine, and can be restored.
  - **Leftovers of apps that are gone** are offered only from the Library folders other than
    Caches (Cleanup's) and the containers (macOS manages sandboxed apps' data; they are offered
    only for an app removed here), only for a name that looks like a bundle ID, that no
    installed or running app (nor another app of the same vendor) could own, and that hasn't
    changed for 30 days. They are Medium risk and labelled "Check first": a command-line tool
    without an app may still use its settings.
  - **Installers** are `.dmg`, `.pkg`, `.mpkg` and `.xip` files directly in Downloads or on the
    Desktop, Low risk.
  - **Bounds.** `AppLeftover { owner }` is in bounds only directly in a leftover folder and when
    its name matches `owner`; this is the one way into Application Support and the containers.
    `Installer` only directly in Downloads or Desktop with an installer extension. Both are
    checked again at preview and approval with the running apps (an open owner app makes its
    leftovers "Not now"), like every other kind. A restore needs only a leftover folder or an
    installer place (the owner isn't recorded).
- **Consequences:** Kleari now changes something outside quarantine: an app bundle, which goes
  to the Trash, never deleted. CONTRIBUTING's safety rule names it. App data is still never
  moved without the user selecting it and approving a preview. Matching by name can miss a
  leftover; it never offers another app's data, Apple's, or anything a running app owns.
