# ADR-024: Moving to the bundle identifier dev.kleari.app

- **Status:** Accepted, by the product owner's decision.
- **Context:** Kleari's bundle identifier was `dev.bakare.kleari`, named after one maintainer.
  The project wants a neutral one, `dev.kleari.app`. macOS keys an app's data folders and
  every permission it was given (Accessibility, Full Disk Access, notifications, folder and
  Automation prompts, Open at login) to the identifier, so changing it would otherwise lose
  the history, the settings and, above all, the quarantine folder, which holds the user's
  files.
- **Decision:**
  - **The data moves at the first launch under the new identifier**, before Tauri starts, so
    nothing of Kleari's has a folder open (`identity.rs`). If
    `~/Library/Application Support/dev.bakare.kleari` exists and the new folder doesn't, it is
    renamed to `~/Library/Application Support/dev.kleari.app`: one rename on the same disk,
    which moves the history, settings and quarantine folder together (quarantined items are
    recorded relative to the folder, so they need no rewriting). WebKit's storage
    (`~/Library/WebKit/<id>`: the appearance and each list's sort and filter) moves the same
    way, if the new one doesn't exist yet. Kleari's other old folders (Caches, HTTPStorages,
    Saved Application State and the Preferences plist) hold only what macOS rebuilds, and are
    deleted; a symbolic link among them is removed, never followed.
  - **Nothing is merged.** If both identifiers have a data folder (an older Kleari run after
    the move made one again), the new one is used and the old one is left as it is. If a
    Kleari with the old identifier is running, nothing moves at that launch and this run uses
    the old folder where it is; so does a run whose rename failed. The next launch tries again.
  - **The old identifier's permissions are reset**, so System Settings doesn't list a second
    "Kleari" that no longer exists, through `/usr/bin/tccutil reset All dev.bakare.kleari`.
    `tccutil` finds the app through LaunchServices, which no longer knows the old identifier
    once the update replaced the bundle (probed: exit 64, `-10814`). Kleari then writes a stub
    bundle with the old identifier into its own `~/Library/Caches/dev.kleari.app` (an
    Info.plist and a two-line shell script that is never run; LaunchServices doesn't register
    a bundle without an executable, and ignores temporary folders), registers it with
    `lsregister -f`, runs the reset, unregisters it with `lsregister -u` and deletes it. Each
    program runs with fixed arguments, no shell and no environment, and is stopped after 10
    seconds.
  - **A one-time sheet asks for the permissions again**, after the first-launch notice and
    setup, never on top of them. It lists only what applies: Accessibility when the Mouse
    settings were on, Full Disk Access, notifications when any notification or the scan
    schedule was on, and Open at login (whether it was on can't be read for the old
    identifier, so the sheet says to turn it on again if it was). Each opens the right
    System Settings pane and is read again when Kleari's window comes back into focus (no
    polling). If the old identifier's permissions couldn't be reset, the sheet says how to
    remove the old entry. Done or Escape closes it for good (a setting in the history).
- **Consequences:** Users of beta 3 or older allow Kleari's permissions once more after this
  update. `lsregister` joins the programs Kleari may run (`pnpm check:network`). A Kleari older
  than beta 4 run again after the move starts empty under the old identifier; its folder is
  never merged back. Development builds share the new identifier and so the same data folder.
