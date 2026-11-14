# ADR-012: Cleanup execution and the quarantine folder

- **Status:** Accepted. Shipped in release builds by the product owner's decision.
- **Context:** ADR-003 sets the cleanup safety model: read-only scan, explained preview,
  explicit approval, backend revalidation, quarantine first, no fall-through to deletion,
  restores that never overwrite, permanent deletion behind its own confirmation. This ADR
  records how Kleari implements it, the first feature that moves or deletes files.
- **Decision:**
  - **What can be offered.** Rust decides, in `cleanup::policy`, from what the scan observed.
    Risk (what you lose) and eligibility (whether Kleari will move it now) are separate.
    - *Eligible:*
      - app caches in `~/Library/Caches` whose app isn't running;
      - package-manager caches (the Developer catalog, and Homebrew, pip, Yarn and similar
        folders in `~/Library/Caches`);
      - logs and crash reports untouched for the chosen age;
      - temporary files untouched for 7 days;
      - Xcode DerivedData (not while Xcode is open), Archives (medium risk) and device
        support;
      - Simulator caches;
      - `node_modules` with a lockfile next to it, or at the root of a workspace that
        includes the project: `pnpm-workspace.yaml` `packages:`, or `workspaces` in
        `package.json`, matched as globs with `!` exclusions. Any other lockfile above
        belongs to another project and doesn't count. The search stops at the repository
        root holding `.git`, the home folder or 8 levels;
      - other build folders of projects with nothing in them open in any app (checked at the scan and again before anything moves; originally, projects without a commit in the last 14 days).
    - *Never moved (inspect only):*
      - Apple's own caches (`com.apple.*`);
      - simulator devices and runtimes (manage them in Xcode);
      - large and old files, whether the Deep scan found them or Storage sent them;
      - anything on a volume other than the quarantine folder's.
    - *Not now:* the owning app, Xcode or the Simulator is running; the project is active;
      `node_modules` without a lockfile needs a check.
    - Build folders in folders added through the panel get the home folder's protections
      too: nothing inside a never-touch folder, Library or the Trash, even if that folder
      itself was added.
    - Every item must also sit at the fixed place its kind allows (`within_bounds`: an exact
      parent such as `~/Library/Caches`, a catalog path, a known build-folder name), strictly
      inside the home folder (or the temporary folder for temp files). Nothing may be in or
      contain `.git`, `.ssh`, `.gnupg`, Keychains, Mail, Messages, Containers, Group
      Containers, Application Support, iCloud or CloudStorage. (Amended by ADR-017: an app's own
      leftover, directly in a Library folder and named for that app, is the one exception.)
  - **Where items come from.** Only the Cleanup scan, the Developer scan or the Storage
    analysis creates items. Each item's real path and identity (device, inode, folder or
    file) stay in Rust, and the frontend refers to it by an opaque hex ID.
  - **Preview.** `create_cleanup_preview` re-reads each selected item from `/`, one component
    at a time without following symlinks, re-runs the policy with the apps running now,
    re-checks its place, and measures it again. Items with a new identity are marked
    *changed since the scan* and left out. The preview binds each remaining item to its
    identity and size.
  - **Approval.** `approve_cleanup_preview` repeats every check for every bound item, with the
    same identity and the same size. If any item differs, nothing moves and the preview must
    be rebuilt. Only then:
    1. The batch and its items are written to the history as `moving`, in one transaction.
       If the history can't be written, nothing moves.
    2. Each item moves by `renameatx_np(RENAME_EXCL)` (Linux CI: `renameat2(RENAME_NOREPLACE)`)
       from its open parent folder into `<app data>/Quarantine/<batch>/<item>`. This is a
       same-volume rename that never replaces anything. A cross-volume move fails (`EXDEV`)
       and the item stays where it was. Nothing is ever copied and then deleted.
    3. The identity is checked immediately before the rename and again in the quarantine
       folder after it. If a swap moved something else, it is moved back; if it can't be,
       the record is updated to the identity actually held.
    4. Each item's outcome is reported and recorded (`held` or `notMoved`). If the check
       after the rename fails, the move is `unconfirmed`: the record stays `moving` and the
       item is never reported or recorded as left in place. Partial results are shown as
       partial.
  - **The quarantine folder** is `~/Library/Application Support/dev.bakare.kleari/
    Quarantine`, created owner-only (0700) and opened without following symlinks. Kleari
    never empties it on its own. There is no retention period or size cap.
  - **Restore** first records the item as `restoring` (if that can't be written, nothing
    moves), then moves it back with the same exclusive rename into its original folder,
    opened from `/` without symlinks. What arrived is checked against the recorded identity:
    a replacement is moved straight back into quarantine, and if that can't be done or
    checked, the outcome is `unconfirmed` and the record stays `restoring`. The real outcome
    is reported even if the history write after it fails. If anything is at the original
    place, the restore is refused (`conflict`). If the folder is gone, it is
    `folderMissing`; if the item isn't in the quarantine folder with its recorded identity,
    `missing`. Each outcome is recorded.
  - **A stored original is checked before it is used.** The history is a file on disk and
    could be edited. Before a restore, and before Show in Finder reveals the original
    place, the recorded path must pass the same place rules as a new item of the recorded
    category (`policy::restorable`: `within_bounds` for that category's kinds, in the home
    folder or the temporary folder, with the never-touch folders excluded). A record that
    fails is refused as `notAllowed` without changing its state, and the listing shows it
    as *unverified*. Large files and simulator devices are never moved, so their categories
    never qualify. Nothing read from the history widens where a restore may go. A build
    folder outside the home folder may go back only into a folder the user chose in the
    folder panel this session (Cleanup's, Developer's, or Quarantine's own), by the same rule
    as at approval (`build_folder_in_added`; the file-system root and non-plain paths never
    count, for approval and restore alike). The added folder it was approved under is
    recorded (`root`, history migration 2) only to say which folder to ask for. Such an item
    is listed as *choose its folder* (`needsFolder`) with a Choose folder… button; a restore
    before that is refused as `needsFolder`.
  - **Choose folder…** calls `choose_restore_folder`, a restore-only grant kept in
    `QuarantineState` until Kleari quits: the folder is never added to a scan. It takes
    the operation lock first; if a move, restore or delete is running it grants nothing and
    reports `operationInProgress`. The page words the recorded folder as what Kleari's
    record says, never as fact, and leaves it out when it is too broad to be a project
    folder (a top-level folder of the disk, the home folder or above, or a volume's own
    folder such as `/Volumes/Disk`). That is decided from the path alone: listing never
    reads a path taken from the history, which could be on a network volume. An edited `root` can only change what Kleari asks
    for; the user's choice in the panel is what allows the restore. Show in Finder stays
    available on pending rows.
  - **Launch reconciliation** settles an unfinished record from what the quarantine folder
    proves (held, or never moved) whatever its original path. It concludes `restored` from
    the original place only when that path passes the same check; otherwise the record stays
    unresolved. No folder is chosen at launch, so an interrupted restore from an added
    folder is listed pending with Choose folder…. Reconciliation runs again, under the
    operation lock, right after a folder is chosen and each time Quarantine is listed while
    nothing is running, so a record whose place comes into scope is never left pending.
  - **Quarantine listing** marks unresolved records as *pending*, and items it can't check
    (the folder can't be inspected, or something else is under the name) as *unverified*.
    Neither can be selected, restored or deleted until they are resolved.
  - **Permanent delete** is separate: a confirmation dialog with an acknowledgement, then
    one item at a time with "Stop after this item". The walk deletes only inside the
    quarantine folder, through handles opened with no-follow on the same volume. It checks
    the recorded identity itself and empties a folder only through the handle verified
    against it, so a replacement put under the item's name isn't emptied. A symlink
    inside is removed as a link, never followed. Read-only folders the item owns (Go's
    module cache) get owner write first. An item that can't be fully deleted stays held and
    is reported.
  - **Delete permanently from Preview** is that same permanent delete, run straight after an
    approval: the preview's "Delete permanently instead…" asks in its own confirmation
    dialog with an acknowledgement, the approval moves the items into quarantine with every
    check above, and then exactly the items it moved and confirmed (each result names the
    quarantine item it became) are deleted one at a time through Quarantine's delete. An
    item it couldn't delete, or a move that couldn't be confirmed, stays in Quarantine. There
    is no second path that deletes a file where it is.
  - **History** (ADR-006): `history.sqlite3` in the app's data folder, owner-only, with
    explicit migrations and `synchronous = FULL`. It stores batches, items (with the
    original path, needed to restore), operation outcomes without paths, and protected-folder
    access answers. A corrupt file, or one from a newer Kleari, is reported and left
    untouched: the version check runs before the journal mode is set. Nothing can move until
    it is resolved. At launch, items left `moving` or `restoring` are resolved only from what
    the folders prove: `held`, definitely `absent` (so `notMoved`), or back at the original
    place (so `restored`). When the folders can't be inspected, the record is kept as it is.
  - **One operation at a time:** approval, restore and permanent delete share one lock.
    Settling unfinished records (after Choose folder…, and when Quarantine is listed) holds
    it too, briefly: an operation that starts meanwhile waits for it instead of failing, and
    a settle is skipped while an operation runs.
  - **Permissions.** Opening a page never triggers a macOS prompt:
    - Downloads, Desktop, Documents and Movies are read only after you choose Grant access,
      which reads the folder once so macOS asks. The answer is remembered.
    - If Full Disk Access is on (proven the way the Storage page checks it, by opening a
      folder only it unlocks), all four count as readable (`fullDiskAccess`), and nothing
      asks.
    - A refusal offers System Settings › Files and Folders.
    - Folders added through the macOS folder panel are granted by that choice.
    - Items macOS keeps private without Full Disk Access are counted, and the page offers to
      open that setting.
  - **Show in Finder** reveals review and quarantine items by Rust's own record of the path,
    and for review items only if the identity is unchanged.
  - **Capabilities:** the seventeen Cleanup and Quarantine commands are granted in the default
    capability.
- **Consequences:**
  - Items on other disks (projects in an added folder on an external drive) are listed but
    can't be quarantined. A per-volume quarantine folder would be a separate decision.
  - Between the last identity check and the rename, a swap could move a different entry
    into quarantine. It is restorable, never deleted, and it is moved back when detected.
  - POSIX has no unlink by handle, so permanent delete keeps two small windows: a file
    swapped between its last `lstat` and `unlinkat`, and an empty folder swapped in just
    before the final `rmdir`. Both need another process of the same account acting on
    Kleari's own quarantine folder at that moment.
  - Workspace patterns are matched iteratively, with adjacent `**` collapsed. A workspace
    with more than 256 patterns, or any pattern over 32 segments, includes nothing: patterns
    are never read in part, so a dropped exclusion can't make a project eligible.
  - A rollback (a swapped entry moved back during a move or a restore) settles the record
    only on proof about the recorded item itself. `notMoved` needs the approved item back at
    its original place and nothing under its name in quarantine; back to `held` needs the
    recorded item in quarantine under its name. Otherwise the outcome is `unconfirmed` and
    the record stays unresolved.
  - Show in Finder for a quarantined item builds its path only from IDs Kleari makes
    (hex digits and dashes, one component each), so a malformed history can't point it
    outside the quarantine folder.
  - A permanent delete that fails part-way, including an entry vanishing during the walk,
    is a failure and the item stays held. Only an item absent before the walk starts is
    recorded as deleted.
  - An unresolved record may stay unresolved if the folders never prove an outcome. Such
    items stay listed (not confirmed, or couldn't verify), with Show in Finder, but restore
    and delete stay unavailable for them.
  - App caches are matched to running apps by bundle identifier only. A cache folder named
    differently from its app's identifier isn't known to be in use. Its risk is low, and it
    can be restored.
  - Quarantined items still use disk space until permanently deleted.
  - Quit requests on the Processes page (ADR-009) are still not recorded. The history now
    exists, so recording them is a follow-up.
  - The Activity page isn't live yet. Outcomes are recorded, but there is no screen for them
    yet.
