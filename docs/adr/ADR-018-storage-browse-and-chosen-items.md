# ADR-018: Storage Browse and items you choose

- **Status:** Accepted, by the product owner's decision. It amends ADR-012, under which large
  and old files were only ever listed to inspect.
- **Context:** The Storage page showed what takes space but stopped at a summary: you could
  see a large folder or file and then had to go to Finder to deal with it. People asked to
  drill into any folder down to single files, choose what to clear, and have it go through
  the same safe path as everything else.
- **Decision:**
  - **Browse** (`storage/explorer.rs`) lists one folder of the home folder at a time, every
    entry measured, read-only: opened through handles without following symlinks, other
    volumes not entered, no file opened, and folders excluded in Settings listed by name only.
    The page gets opaque IDs; paths and identities stay in Rust, and a folder replaced since
    it was listed is refused. A new listing stops one still running.
  - **Choosing** sends entries to Cleanup's review as `Kind::UserChosen` (category "Chosen by
    you", Medium risk, unknown certainty). It is in bounds only inside the home folder,
    outside the places Kleari never touches (ADR-012's list), not a top-level home folder
    itself (Desktop, Documents, Downloads, Library, Movies, Music, Pictures, Public,
    Applications, the Trash), in the Library only below Caches, Logs or Developer, never in the
    Trash, and never inside a package (`.app`, `.photoslibrary`, …; the package itself may be
    chosen).
  - **While open, never.** A chosen item is held ("Not now") while anything inside it is open
    in an app, read when it is sent and again at preview and approval (fail closed when it
    can't be read), the same check as active projects.
  - **Large files from Storage** ("Add to review") are chosen items when they are in these
    bounds, and stay inspect-only elsewhere. Large and old files a Cleanup scan finds by itself
    stay inspect-only: Kleari doesn't offer to move what you didn't pick.
  - As always, nothing moves until you approve a preview, and then only into quarantine.
- **Consequences:** You can now clear your own files through Kleari, so the review says plainly
  that Kleari doesn't know what they are. The bounds keep macOS's layout, app data, sync
  folders' roots and package internals out of reach.
