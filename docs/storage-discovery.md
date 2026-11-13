# Storage discovery: design note

**Status:** Approved by the product owner (decisions recorded below); implemented on
`feat/storage-discovery`.

- The backend (grants, scanner and query) is still in place, but its command permissions
  aren't granted while no screen uses it.
- Its screen was retired when release builds moved to the v2 interface. The Storage page now
  uses the [fixed-location analysis](#storage-page-analysis) below, which reuses this
  scanner's traversal.
- The user flow below describes the retired screen. Picking a folder will return with
  Cleanup.

## Goal

Let the user choose a folder and see, read-only, what uses space inside it: total size, a
breakdown by immediate subfolder and the largest files. This is the first "scan freely"
capability. It performs **no** cleanup, deletion, quarantine, moving or writing of any
kind, and it adds nothing to cleanup eligibility (ADR-003).

## User flow

1. **Storage** is a new sidebar destination. Its first view explains that the scan is
   read-only and asks the user to choose a folder (SCREENS.md "Storage explorer").
2. **Choose folder…** opens the native macOS folder picker. Cancelling the picker changes
   nothing.
3. The scan starts immediately on the chosen folder. It shows what is being read, a running
   count of files and bytes, and a **Cancel** button.
4. Results show:
   - scope: the folder name and when it was scanned
   - coverage: bytes counted, plus items skipped as unreadable, symlinks not followed and
     other volumes not entered, each with a count, so unknown space is never shown as zero
   - a solid-surface table of the top-level breakdown by immediate subfolder
   - a table of the largest files
5. **Scan again** and **Choose another folder** are the only actions. There is no item
   action, and selecting a row does nothing (DESIGN.md).

## Security and safety design

### Backend-owned grants; the frontend never sends paths
- The picker is opened **from Rust**, not the WebView.
- Rust stores the chosen root, canonicalized, with its device and inode, under a
  sequential **grant ID** (`g1`, `g2`, …), and returns only `{ grantId, displayName }` to
  the UI. The ID doesn't need to be unguessable: it can only name a grant the backend
  created for a folder the user picked.
- `start_scan(grantId)` accepts only a grant ID. An unknown ID is rejected. The frontend
  cannot name an arbitrary path, so a compromised WebView cannot scan where the user did
  not choose. This follows the rule that Rust decides everything that touches files and the
  interface never sends a path to act on ([CONTRIBUTING.md](../CONTRIBUTING.md)).
- Grants live in memory only, for the app session; they are not persisted.
- Each scan opens the root with no-follow semantics and checks the device and inode **on the
  opened handle**. If the root was replaced, deleted or swapped for a symlink, the scan fails
  closed with `rootChanged`.

### Read-only, race-free traversal
- **Folders are opened by handle, never re-resolved by path** (`storage/fsdir.rs`). For each
  child the scanner:
  1. reads its metadata relative to the open parent without following symlinks
     (`fstatat(AT_SYMLINK_NOFOLLOW)`)
  2. opens it relative to the parent (`openat(O_NOFOLLOW | O_DIRECTORY)`)
  3. checks the opened handle's device and inode match step 1

  A folder swapped for a symlink at any point is either refused at open or fails the
  identity check (counted as unreadable); it is never followed. Path-based `read_dir` would
  re-resolve the path and could follow such a swap.
- Only metadata and directory listing are used.
  - File contents are never opened.
  - Nothing is ever written, created, renamed or deleted.
  - Modification and change times are never altered. Listing a directory may update that
    directory's **access time**, as any listing does; the no-writes test therefore checks
    every other attribute.
- **Symlinks are never followed.** Each one is counted under "symlinks not followed" and
  contributes 0 bytes. This rules out escaping the chosen folder.
- **The scan stays on one volume.** A directory on a different device is not entered and is
  counted under "other volumes not entered" (e.g. mounted disks, network shares, Time
  Machine). This avoids surprise network traffic and endless scans.
- **Hard links are counted once**, deduplicated by (device, inode), so totals aren't
  inflated. Up to 1,000,000 identities are remembered. Beyond that, further hard-linked
  files are counted and reported as "may be counted more than once".
- **App bundles and packages** (`.app`, `.photoslibrary`, …) are scanned as ordinary
  folders; their sizes count toward their parent. They are not given special treatment
  in this feature.
- **Size reported:** allocated size on disk (`st_blocks × 512`), which matches what Finder
  and `du` report. Logical size can differ for sparse or compressed files. The UI says
  "on disk".
- **Unreadable entries:** a permission error never aborts the scan.
  - The entry is counted under "unreadable", and its size is unknown, not zero.
  - A top-level row containing unreadable items shows its size as "≥ x", or "Unknown" if
    nothing in it could be read.
  - The total says "At least" whenever anything was unreadable or too deep.
  - The scan never escalates or requests broader access.
  - macOS may show its own privacy prompt for protected folders (Desktop, Documents,
    Downloads). Choosing the folder in the picker normally grants that access; anything
    still denied is reported as unreadable.

### Cancellation, progress and limits
- The scan runs on a background thread with a cancellation token. **Cancel** stops it
  within one directory entry, and the results are marked "Cancelled — partial".
- **Progress** is streamed over a typed Tauri channel at most every 250 ms, including while a
  single large folder is listed: files seen, bytes counted, and the current folder relative
  to the root. Screen readers hear a rounded count ("About 1,000 files so far"), which
  changes only a few times per order of magnitude.
- Only one scan runs at a time. Starting another while one is running is rejected with
  `scanInProgress`; it never silently cancels the running scan. The UI disables starting
  while a scan runs.
- **Bounded memory and payloads.** Every limit is a field of `ScanLimits`:

  | What | Limit | Beyond the limit |
  |---|---|---|
  | Open folders along the current path (depth-first) | 128, including the root | Deeper folders are counted as "nested too deeply to enter"; size unknown |
  | Top-level rows | 100 largest | The rest are combined into one "N other items" row (filterable) |
  | File index for filtering | Up to 250,000 files, and at most 64 MiB of path text | Larger files are favoured (see the selection policy below); the UI says filters search N of M files |
  | Hard-link identities | 1,000,000 | Reported as "may be counted more than once" |
  | Files per result | 50 | — |

  The walk keeps only the folders on the current path open, so memory grows with depth,
  not with the total number of folders. It is iterative, so deep nesting can't overflow the
  stack. The full listing is never sent to the UI.
- **Filtering runs off the main thread.** `query_scan` is asynchronous and filters the
  shared index on a worker thread. ASCII name searches compare without allocating.

### Known limitation: same-device aliases

The "other volumes" check compares device IDs. A mount that presents part of the *same*
device at another path (for example a bind or nullfs mount) would not be recognized as an
alias, so its contents could be counted twice. Stock macOS doesn't create such mounts in
user folders. The system's firmlinks (for example `/Users` on the Data volume) have a
different device ID from the system volume. Scanning `/` therefore stops at them and
reports them as other volumes; choose your home folder instead.

### Privacy
- Paths are shown **relative to the chosen folder** and only on screen.
- Names that aren't valid UTF-8 are shown with each invalid byte as `\x{XX}`, so distinct
  names never collapse into the same text. Top-level rows are filtered by a stable row
  `id`, never by name.
- Results are in memory only, never stored, logged or sent anywhere.
- Error messages use stable codes, not raw OS strings with full paths.

## IPC contract (typed, validated in Rust and TS)

| Command | Input | Output |
|---|---|---|
| `choose_scan_folder` | none | `{ status: "chosen", grantId, displayName }` or `{ status: "cancelled" }` |
| `start_scan` | `grantId`, `onProgress` channel of `{ files, bytesOnDisk, current }` | `ScanSummary` (status `completed` or `cancelled`), or a failure code |
| `cancel_scan` | `grantId` | none |
| `query_scan` | `grantId`, `filter { name?, category?, topLevel? (row id), minBytes? }` | `QueryResult` |

`ScanSummary` contains:
- `grantId`, `displayName`, `status`, `startedAtMs`, `durationMs`
- `totals { bytesOnDisk, files, directories }`
- `coverage { unreadable, symlinksSkipped, otherVolumesSkipped, tooDeep, hardLinksUnverified }`
- `topLevel [{ id, name, kind: folder | files | others, bytesOnDisk, files, unreadable, items }]`
  (at most 101 rows)
- `indexedFiles`
- `largest: QueryResult`

`QueryResult` is `{ files [{ relativePath, bytesOnDisk, category, topLevel (row id) }], matchedCount, indexTruncated }`,
with at most 50 files.

Failure codes: `grantNotFound`, `rootChanged`, `rootUnreadable`, `scanInProgress`,
`noScanResult`, `invalidFilter`, `pickerUnavailable`.

## Testing

All filesystem tests use disposable temporary directories (testing-strategy.md); they
never use personal files. They cover:
- totals and the top-level breakdown on a synthetic tree
- symlinks to outside the root are not followed, including symlink loops
- hard links are counted once
- unreadable directories (permission 000) are counted, not fatal
- cancellation mid-scan gives partial results
- a root replaced after the grant gives `rootChanged`
- an unknown grant ID is rejected
- a **no-writes assertion**: a snapshot of the tree's metadata before and after the scan
  is identical
- deep nesting doesn't overflow the stack
- largest-files bounding and ordering
- frontend: contract validation, and every view state (empty, scanning, cancelled,
  partial, failed, completed)

## Filters and the file index

The largest-files list has four filters, which combine with AND:
- **name search:** case-insensitive, over the relative path
- **file type:** by extension; one of Video, Image, Audio, Archive, Disk image, Document,
  Code and build, Other
- **top-level folder:** a row `id` from the breakdown, including the combined row
- **minimum size**

Filters **rank within the match**: the list always shows the 50 largest files that match,
not the overall top 50 narrowed down.

To re-rank without rescanning the disk, the backend keeps an in-memory **index of up to
250,000 files, holding at most 64 MiB of path text**, from the last scan: relative path,
size, type and top-level folder.

**Selection policy:** a file is admitted only if room can be made by evicting strictly
smaller files, and that is decided before anything is evicted, so a rejected file never
displaces a kept one. Larger files are therefore favoured, but the index is not strictly
"the N largest": when paths are long, a large file can be left out to stay within the
byte budget. When anything is left out, the UI says filters search N of M files, the
largest Kleari could keep within its memory limit. The index is dropped when a new scan
starts and is never stored.

`query_scan(grantId, filter)` returns the top 50 matches and the match count. Rust
validates the filter:
- the name is at most 200 characters
- the type comes from a fixed set
- the top-level filter must be an existing row `id`
- the minimum size is a non-negative integer

## Decisions (product owner)

1. **Folder picker:** macOS `NSOpenPanel`, called directly through `objc2-app-kit` (already
   in the dependency tree; only feature flags are added). No new crates.
2. **Placement:** a new **Storage** sidebar destination.
3. **Hidden files and folders:** included in totals and shown in the tables.
4. **Results:** the 50 largest files, with name, type, top-level folder and minimum-size
   filters that rank within the match, over an index of up to 250,000 files. A 64 MiB
   path-text cap was added after review to bound memory.
5. **Delivery:** one PR.

Grant IDs are sequential (`g1`, `g2`, …) rather than random. The frontend can only refer to
grants the backend created for folders the user picked, so an ID doesn't need to be
unguessable.

## Storage page analysis

The v2 Storage page measures the whole startup disk instead of one chosen folder. It shares
this scanner's traversal (`fsdir`: folders opened by handle relative to their open parent,
no symlinks followed, no other devices entered, hard links counted once). It doesn't share
grants: the locations are fixed in Rust, so the frontend still never sends a path. The
command contract is in [architecture.md](architecture.md#implemented-storage-page-data-read-only).

### Decisions (product owner)

1. **Scope:** fixed locations rather than chosen folders: Applications, the home folder,
   `/System` and `/usr`, and `/Library`, `/opt`, `/private/var` and `/usr/local`.
   - macOS may ask before Kleari reads protected folders (Desktop, Documents, Downloads,
     other apps' data).
   - Whatever it can't read counts as unreadable and marks sizes as minimums.
2. **Full Disk Access:** optional.
   - The page detects it and, when it is off, offers a button that opens the Full Disk
     Access pane.
   - The first Analyze of a visit while it's off first explains what it would add, with
     **Open System Settings** and **Analyze without it**. That is the most macOS allows:
     there is no API to request Full Disk Access.
   - Kleari can't turn it on.
   - In development (`pnpm tauri dev`), Kleari runs as a bare binary, not an app bundle,
     so it isn't listed in that pane. macOS applies the permission of the app that launched
     it (the terminal or editor). To test the granted state, grant that app, then restart it,
     and turn it off again afterwards.
3. **Unbuilt features are hidden:** the quarantine and review chips, "Add to review", "Scan in
   Cleanup" and the "Reviewable in Cleanup" tag stay hidden until Cleanup and Quarantine exist.
4. **Available:** Finder's figure (free plus purgeable), with purgeable space shown separately
   as an estimate.
5. **Dropped from the design:**
   - The local Time Machine snapshots row: snapshot sizes need administrator access, and
     macOS already counts snapshots in purgeable space.
   - The "Sorting into categories" step: sorting happens while sizes are counted, so it has
     no duration of its own.

### Counting each byte once

On APFS, `/System/Volumes/Data` and the firmlinked folders (`/Applications`, `/Users`,
`/private/var/vm`, …) report the same device as `/`, so the other-volume check alone doesn't
stop double counting. Each location therefore has an explicit exclusion list:

- `/System/Volumes` is not walked; it holds the other volumes, including the Data volume.
- `/usr/local` is not walked under `/usr`; it is walked as its own location.
- `/private/var/vm` is not walked; it holds the sleep image, shown with swap as space macOS
  manages.

Swap files are on the VM volume, which no location reaches.

### Categories by location

`storage::analysis::classify` assigns each folder a category from where it is. A folder no
rule names inherits its parent's category.

| Location | Category |
|---|---|
| `/Applications`, `~/Applications` | Applications |
| `~/Library/Containers`, `~/Library/Group Containers` (except Docker's) | Applications (each app's sandbox data) |
| A folder directly in `~/Library/Application Support`, `~/Library/Caches`, `/Library/Application Support` or `/Library/Caches` named after an installed app: its name, its bundle identifier or that with a suffix (`com.google.Chrome.helper`), or the identifier's vendor (`Google`, `JetBrains`; never `Apple`) | Applications (app data, as macOS's Storage settings count it) |
| Home folder | User files |
| `~/Library` | Other (app data) |
| `~/Library/Caches`, `~/Library/Logs`, `~/.cache` | Caches and temporary |
| `~/Library/Developer`, Docker's container and group container, developer dot folders at the top of the home folder (`.npm`, `.cargo`, `.rustup`, …), and project folders outside Library (`node_modules`, `.venv`, `DerivedData`, `Pods`, …) | Developer files |
| Other dot folders at the top of the home folder | Other |
| `~/.Trash` | User files |
| `/System`, `/usr`, `/private/var` | System files |
| `/private/var/folders`, `/private/var/tmp`, `/private/var/log`, `/Library/Caches`, `/Library/Logs` | Caches and temporary |
| `/Library` | Other |
| `/Library/Developer` | Developer files |
| `/opt`, `/usr/local` | Developer files |

**Applications and macOS's Storage settings.** The product owner chose to count apps' data
with them, as macOS does.
- **What Kleari reads:** the installed apps' names and bundle identifiers, from each
  bundle's `Contents/Info.plist`. This is the only file content the analysis reads.
  - It is app metadata, opened without following symlinks, at most 256 KB per file and 2,000
    bundles (`analysis::apps`).
  - Like the walk, indexing never leaves an Applications folder's volume: a bundle,
    `Contents` or plist on another device is skipped.
  - It touches nothing at or below a mount the mount table reports as non-local, including
    the folder itself.
  - The table is read again before each folder and each bundle. If it can't be read,
    indexing stops there.
  - A volume mounted between a check and the next lookup is still looked up once, and the
    device checks then stop the descent. This is the same residual as the startup-item check
    and bundle resolution.
- **Publisher folders:** a publisher folder (`Google`) can hold data from that publisher's
  other products too. The Applications description says so.
- **Why the figures can differ:** macOS's own rules are private, so they can still differ.
  - On the development Mac, `/Applications` alone was 45.2 GB. With the apps' data matched,
    Applications came to 98.6 GB; System Settings showed 178.48 GB.
  - Docker's 22 GB disk image stays under Developer files on purpose.
  - Data in folders that match no installed app stays under Other.

Each category lists its three largest items: the folder a rule named, or each folder inside a
broad one (each app, each folder in the home folder, each folder in `~/Library`). An item's
size counts only its files in that category, so a project folder's own size excludes the
`node_modules` inside it.

### What the numbers mean

- **Sizes are allocated bytes**, as in the folder scan.
  - APFS clones share storage but are counted for each copy, so a total can be higher than
    the space really used.
- **Anything not counted makes totals minimums.** That covers entries that couldn't be read,
  folders past 128 levels, and hard links seen after the 1,000,000-identity limit (these
  aren't counted, since they may duplicate a file already counted).
  - The folders above them are marked "at least".
  - Every category they could have counted toward is marked "at least", together with its share
    of used space.
  - For a folder that couldn't be opened, that means its own category plus every category a
    rule can assign inside it (`classify::possible_below`). For example, an unreadable home
    folder marks User files, Other, Caches and Developer files, and an unreadable `~/Documents`
    also marks Developer files, since a project's `node_modules` could be inside.
- **Used space the analysis didn't count is shown as "Not measured".** It isn't spread over
  the categories. It includes:
  - folders that need Full Disk Access
  - other users' files
  - cloud folders
  - macOS's own volumes (Preboot, Recovery, VM)
- **On the development Mac without Full Disk Access**, the analysis took about 70 s:
  - 281 GB was counted, against 383 GB used.
  - 1,010 entries couldn't be read (for example `~/Library/Mail`, `Messages` and `Biome`).
  - Locations outside the list were small: `/Users/Shared` 63 MB, `/private/tmp` 200 MB.
- **Bounds** (`analysis::Limits`):
  - 128 open levels
  - 1,000,000 hard-link identities
  - a tree of 4 levels × 12 folders of at least 100 MB per location
  - 50 large files
  - 3 items per category
  - The result is about 94 KB of JSON on the development Mac.
