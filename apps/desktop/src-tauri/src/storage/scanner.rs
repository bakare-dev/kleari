//! Read-only directory scan. Folders are opened relative to their open parent with no-follow
//! semantics (see `fsdir`), so a symlink, including one swapped in during the scan, is never
//! followed; other volumes are not entered. No file is opened, written, renamed or deleted.
//!
//! Memory is bounded: the walk is depth-first and holds only the open folders on the current
//! path (at most `max_depth`), the top-level breakdown keeps `top_level` rows plus one
//! aggregate row, the file index keeps the `index` largest files, and hard-link tracking is
//! capped at `hard_links` identities.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::ffi::{CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::category::{category_of, FileCategory};
use super::fsdir::{display_name, DirHandle, FileStat};

/// Bounds for one scan. `Default` gives the production values.
#[derive(Debug, Clone, Copy)]
pub struct ScanLimits {
    /// Largest files kept for filtering.
    pub index: usize,
    /// Total bytes of relative-path text the index may hold. Paths can be up to ~32 KB at the
    /// depth limit, so the file count alone does not bound memory.
    pub index_bytes: usize,
    /// Top-level rows kept before the rest are combined into one row.
    pub top_level: usize,
    /// Folders open at once along the current path, including the root.
    pub max_depth: usize,
    /// Hard-link identities remembered for de-duplication.
    pub hard_links: usize,
    pub progress_interval: Duration,
}

impl Default for ScanLimits {
    fn default() -> Self {
        Self {
            index: 250_000,
            index_bytes: 64 * 1024 * 1024,
            top_level: 100,
            max_depth: 128,
            hard_links: 1_000_000,
            progress_interval: Duration::from_millis(250),
        }
    }
}

/// Identity of the chosen root, captured when the user picks it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootIdentity {
    pub dev: u64,
    pub ino: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanError {
    /// The root no longer exists, is not a directory, is a symlink, or has a different identity.
    RootChanged,
    /// The root itself could not be opened or listed.
    RootUnreadable,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub bytes_on_disk: u64,
    pub files: u64,
    pub directories: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Coverage {
    /// Entries or folders that could not be read (or changed while being read); their size
    /// is unknown, not zero.
    pub unreadable: u64,
    pub symlinks_skipped: u64,
    pub other_volumes_skipped: u64,
    /// Folders nested deeper than `max_depth`, not entered.
    pub too_deep: u64,
    /// Hard-linked files seen after the tracking limit; they may be counted more than once.
    pub hard_links_unverified: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopLevelKind {
    Folder,
    /// Files directly inside the chosen folder, grouped into one row.
    Files,
    /// Top-level items beyond the row limit, combined into one row.
    Others,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopLevel {
    /// Display name (invalid UTF-8 escaped); empty for `Files` and `Others`.
    pub name: String,
    pub kind: TopLevelKind,
    pub bytes_on_disk: u64,
    pub files: u64,
    pub unreadable: u64,
    /// Top-level items this row covers (1, or more for `Others`).
    pub items: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    pub relative_path: String,
    pub bytes_on_disk: u64,
    pub category: FileCategory,
    /// Index into `ScanOutcome::top_level`.
    pub top_level: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanOutcome {
    pub cancelled: bool,
    pub totals: Totals,
    pub coverage: Coverage,
    /// Largest first, with any `Others` row last.
    pub top_level: Vec<TopLevel>,
    /// Largest files, descending by size.
    pub index: Vec<IndexedFile>,
    pub index_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub files: u64,
    pub bytes_on_disk: u64,
    /// Current folder relative to the root; empty for the root itself.
    pub current: String,
}

/// Keeps a size-favoured subset of files within a count limit and a path-text byte budget:
/// a file is admitted only by evicting strictly smaller ones (see [`Indexer::offer`]), so it
/// is not strictly the `limit` largest when paths are long. Ties keep the earlier file, so
/// results are deterministic for a given traversal.
struct Indexer {
    limit: usize,
    /// Min-heap of (size, newer-is-smaller sequence, slot).
    heap: BinaryHeap<Reverse<(u64, Reverse<u64>, usize)>>,
    slots: Vec<Option<IndexedFile>>,
    /// Slots freed by evictions, reused before growing `slots`.
    free: Vec<usize>,
    /// Path bytes held, bounded by `byte_budget`.
    bytes: usize,
    byte_budget: usize,
    seq: u64,
    truncated: bool,
}

impl Indexer {
    fn with_limits(limit: usize, byte_budget: usize) -> Self {
        Self {
            limit,
            heap: BinaryHeap::new(),
            slots: Vec::new(),
            free: Vec::new(),
            bytes: 0,
            byte_budget,
            seq: 0,
            truncated: false,
        }
    }

    fn evict(&mut self, slot: usize) {
        if let Some(file) = self.slots.get_mut(slot).and_then(Option::take) {
            self.bytes -= file.relative_path.len();
            self.free.push(slot);
        }
    }

    fn cost_of(&self, slot: usize) -> usize {
        self.slots
            .get(slot)
            .and_then(Option::as_ref)
            .map_or(0, |file| file.relative_path.len())
    }

    /// Offers a file under both the count and byte limits. A file is admitted only if room
    /// can be made by evicting strictly smaller files; admission is decided *before* anything
    /// is evicted, so a rejected file never costs a kept one its place. Larger files are
    /// favoured, but with long paths a large file can be left out to stay within budget.
    fn offer(&mut self, file: IndexedFile) {
        let key = (file.bytes_on_disk, Reverse(self.seq));
        self.seq += 1;
        let cost = file.relative_path.len();
        if self.limit == 0 || cost > self.byte_budget {
            self.truncated = true;
            return;
        }
        // Tentatively take the smallest entries until the file would fit.
        let mut candidates = Vec::new();
        let mut freed = 0;
        while self.heap.len() >= self.limit || self.bytes - freed + cost > self.byte_budget {
            match self.heap.peek() {
                Some(&Reverse((size, order, slot))) if (size, order) < key => {
                    freed += self.cost_of(slot);
                    candidates.extend(self.heap.pop());
                }
                _ => {
                    // Only entries at least as large remain: reject, restoring the candidates.
                    self.heap.extend(candidates);
                    self.truncated = true;
                    return;
                }
            }
        }
        if !candidates.is_empty() {
            self.truncated = true;
        }
        for Reverse((_, _, slot)) in candidates {
            self.evict(slot);
        }
        let slot = match self.free.pop() {
            Some(slot) => {
                self.slots[slot] = Some(file);
                slot
            }
            None => {
                self.slots.push(Some(file));
                self.slots.len() - 1
            }
        };
        self.bytes += cost;
        self.heap.push(Reverse((key.0, key.1, slot)));
    }

    fn into_sorted(self) -> (Vec<IndexedFile>, bool) {
        let mut order: Vec<(u64, Reverse<u64>, usize)> =
            self.heap.into_iter().map(|Reverse(entry)| entry).collect();
        // Largest first; among equal sizes, earlier files first.
        order.sort_by_key(|&(size, sequence, _)| Reverse((size, sequence)));
        let mut slots = self.slots;
        let sorted = order
            .into_iter()
            .filter_map(|(_, _, slot)| slots.get_mut(slot).and_then(Option::take))
            .collect();
        (sorted, self.truncated)
    }
}

/// Keeps the `limit` largest finished top-level rows; the rest are combined into one row.
struct TopRows {
    limit: usize,
    /// Min-heap of (size, row id) over kept rows.
    heap: BinaryHeap<Reverse<(u64, u32)>>,
    kept: HashMap<u32, TopLevel>,
    others: Option<TopLevel>,
}

impl TopRows {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            heap: BinaryHeap::new(),
            kept: HashMap::new(),
            others: None,
        }
    }

    fn fold_into_others(&mut self, row: &TopLevel) {
        let others = self.others.get_or_insert_with(|| TopLevel {
            name: String::new(),
            kind: TopLevelKind::Others,
            bytes_on_disk: 0,
            files: 0,
            unreadable: 0,
            items: 0,
        });
        others.bytes_on_disk = others.bytes_on_disk.saturating_add(row.bytes_on_disk);
        others.files += row.files;
        others.unreadable += row.unreadable;
        others.items += row.items;
    }

    fn finish_row(&mut self, id: u32, row: TopLevel) {
        if self.kept.len() < self.limit {
            self.heap.push(Reverse((row.bytes_on_disk, id)));
            self.kept.insert(id, row);
            return;
        }
        let Some(&Reverse((smallest, smallest_id))) = self.heap.peek() else {
            self.fold_into_others(&row);
            return;
        };
        if row.bytes_on_disk > smallest {
            self.heap.pop();
            if let Some(evicted) = self.kept.remove(&smallest_id) {
                self.fold_into_others(&evicted);
            }
            self.heap.push(Reverse((row.bytes_on_disk, id)));
            self.kept.insert(id, row);
        } else {
            self.fold_into_others(&row);
        }
    }
}

/// Captures the identity of a directory chosen by the user; `None` if it is not one
/// (a symlink to a directory is not accepted as a root).
pub fn root_identity(path: &Path) -> Option<RootIdentity> {
    let (_, stat) = DirHandle::open_root(path).ok()?;
    Some(RootIdentity {
        dev: stat.dev,
        ino: stat.ino,
    })
}

struct Frame {
    dir: DirHandle,
    /// Top-level row this folder counts toward; `None` for the root.
    row: Option<u32>,
    /// Path relative to the root, for display.
    relative: String,
}

/// A row being filled while its folder (or the root's files) is walked.
struct OpenRow {
    id: u32,
    row: TopLevel,
}

struct Walker {
    limits: ScanLimits,
    root_dev: u64,
    totals: Totals,
    coverage: Coverage,
    next_row: u32,
    /// The top-level folder currently being walked.
    current_row: Option<OpenRow>,
    /// "Files in this folder", open for the whole scan.
    files_row: Option<OpenRow>,
    rows: TopRows,
    indexer: Indexer,
    seen_links: HashSet<(u64, u64)>,
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

impl Walker {
    fn new_row(&mut self, name: String, kind: TopLevelKind) -> OpenRow {
        let id = self.next_row;
        self.next_row += 1;
        OpenRow {
            id,
            row: TopLevel {
                name,
                kind,
                bytes_on_disk: 0,
                files: 0,
                unreadable: 0,
                items: 1,
            },
        }
    }

    fn row_mut(&mut self, id: u32) -> Option<&mut TopLevel> {
        [self.current_row.as_mut(), self.files_row.as_mut()]
            .into_iter()
            .flatten()
            .find(|open| open.id == id)
            .map(|open| &mut open.row)
    }

    fn mark_unreadable(&mut self, row: Option<u32>) {
        self.coverage.unreadable += 1;
        if let Some(row) = row.and_then(|id| self.row_mut(id)) {
            row.unreadable += 1;
        }
    }

    /// A top-level entry that couldn't be read still gets a row, so it is visible with an
    /// unknown size rather than missing.
    fn unreadable_top_level(&mut self, name: String) {
        let mut open = self.new_row(name, TopLevelKind::Folder);
        open.row.unreadable = 1;
        self.coverage.unreadable += 1;
        self.rows.finish_row(open.id, open.row);
    }

    fn count_file(
        &mut self,
        frame_row: Option<u32>,
        relative: String,
        name: &OsStr,
        stat: &FileStat,
    ) {
        let row = match frame_row {
            Some(row) => row,
            None => {
                if self.files_row.is_none() {
                    self.files_row = Some(self.new_row(String::new(), TopLevelKind::Files));
                }
                self.files_row.as_ref().map_or(0, |open| open.id)
            }
        };
        if stat.nlink > 1 {
            let identity = (stat.dev, stat.ino);
            if self.seen_links.contains(&identity) {
                // Another hard link to a file already counted.
                return;
            }
            if self.seen_links.len() < self.limits.hard_links {
                self.seen_links.insert(identity);
            } else {
                self.coverage.hard_links_unverified += 1;
            }
        }
        let bytes = stat.blocks.saturating_mul(512);
        self.totals.files += 1;
        self.totals.bytes_on_disk = self.totals.bytes_on_disk.saturating_add(bytes);
        if let Some(entry) = self.row_mut(row) {
            entry.files += 1;
            entry.bytes_on_disk = entry.bytes_on_disk.saturating_add(bytes);
        }
        self.indexer.offer(IndexedFile {
            relative_path: relative,
            bytes_on_disk: bytes,
            category: category_of(Path::new(name)),
            // Provisional row id; mapped to a final position in `finish`.
            top_level: usize::try_from(row).unwrap_or(usize::MAX),
        });
    }

    /// Visits one entry of `frame`. Returns a child folder to descend into, if any.
    fn visit(&mut self, frame: &Frame, name: &CStr, depth: usize) -> Option<Frame> {
        let os_name = OsStr::from_bytes(name.to_bytes());
        let shown = display_name(os_name);
        let at_root = frame.row.is_none();
        let Ok(stat) = frame.dir.stat_child(name) else {
            if at_root {
                self.unreadable_top_level(shown);
            } else {
                self.mark_unreadable(frame.row);
            }
            return None;
        };
        if stat.is_symlink() {
            self.coverage.symlinks_skipped += 1;
            return None;
        }
        if !stat.is_dir() {
            // Regular files and other non-directory entries (sockets, FIFOs) use disk space too.
            self.count_file(frame.row, join(&frame.relative, &shown), os_name, &stat);
            return None;
        }
        if stat.dev != self.root_dev {
            self.coverage.other_volumes_skipped += 1;
            return None;
        }
        if depth >= self.limits.max_depth {
            self.coverage.too_deep += 1;
            if let Some(row) = frame.row.and_then(|id| self.row_mut(id)) {
                row.unreadable += 1;
            }
            return None;
        }
        // Open relative to the parent with no-follow, then confirm it is the folder just
        // checked: a swap in between is refused or detected, never followed.
        let opened = frame.dir.open_child(name).ok().filter(|(_, opened)| {
            opened.is_dir() && opened.dev == stat.dev && opened.ino == stat.ino
        });
        let Some((dir, _)) = opened else {
            if at_root {
                self.unreadable_top_level(shown);
            } else {
                self.mark_unreadable(frame.row);
            }
            return None;
        };
        self.totals.directories += 1;
        let row = match frame.row {
            Some(row) => row,
            None => {
                let open = self.new_row(shown.clone(), TopLevelKind::Folder);
                let id = open.id;
                self.current_row = Some(open);
                id
            }
        };
        Some(Frame {
            dir,
            row: Some(row),
            relative: join(&frame.relative, &shown),
        })
    }

    fn finish(mut self, cancelled: bool) -> ScanOutcome {
        for open in [self.current_row.take(), self.files_row.take()]
            .into_iter()
            .flatten()
        {
            self.rows.finish_row(open.id, open.row);
        }
        let mut kept: Vec<(u32, TopLevel)> = self.rows.kept.into_iter().collect();
        kept.sort_by(|(_, a), (_, b)| {
            b.bytes_on_disk
                .cmp(&a.bytes_on_disk)
                .then_with(|| a.name.cmp(&b.name))
        });
        let position: HashMap<u32, usize> = kept
            .iter()
            .enumerate()
            .map(|(index, (id, _))| (*id, index))
            .collect();
        let mut top_level: Vec<TopLevel> = kept.into_iter().map(|(_, row)| row).collect();
        let others_position = self.rows.others.map(|others| {
            top_level.push(others);
            top_level.len() - 1
        });

        let (mut index, index_truncated) = self.indexer.into_sorted();
        for file in &mut index {
            let id = u32::try_from(file.top_level).unwrap_or(u32::MAX);
            file.top_level = position
                .get(&id)
                .copied()
                .or(others_position)
                .unwrap_or_default();
        }
        ScanOutcome {
            cancelled,
            totals: self.totals,
            coverage: self.coverage,
            top_level,
            index,
            index_truncated,
        }
    }
}

/// Scans `root` read-only with the production [`ScanLimits`].
pub fn scan(
    root: &Path,
    expected: RootIdentity,
    cancel: &AtomicBool,
    progress: impl FnMut(Progress),
) -> Result<ScanOutcome, ScanError> {
    scan_with(root, expected, cancel, progress, ScanLimits::default())
}

/// Scans `root` read-only. `expected` is the identity captured when the user chose the root;
/// it is checked on the opened handle, and a mismatch fails closed. `progress` is called at
/// most every `limits.progress_interval`, including within a single large folder.
pub fn scan_with(
    root: &Path,
    expected: RootIdentity,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
    limits: ScanLimits,
) -> Result<ScanOutcome, ScanError> {
    let (root_dir, stat) = match DirHandle::open_root(root) {
        Ok(opened) => opened,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(ScanError::RootUnreadable)
        }
        Err(_) => return Err(ScanError::RootChanged),
    };
    if stat.dev != expected.dev || stat.ino != expected.ino {
        return Err(ScanError::RootChanged);
    }

    let mut walker = Walker {
        limits,
        root_dev: expected.dev,
        totals: Totals::default(),
        coverage: Coverage::default(),
        next_row: 0,
        current_row: None,
        files_row: None,
        rows: TopRows::new(limits.top_level),
        indexer: Indexer::with_limits(limits.index, limits.index_bytes),
        seen_links: HashSet::new(),
    };
    let mut frames = vec![Frame {
        dir: root_dir,
        row: None,
        relative: String::new(),
    }];
    let mut last_progress = Instant::now();

    while let Some(frame) = frames.last_mut() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(walker.finish(true));
        }
        if last_progress.elapsed() >= limits.progress_interval {
            last_progress = Instant::now();
            progress(Progress {
                files: walker.totals.files,
                bytes_on_disk: walker.totals.bytes_on_disk,
                current: frame.relative.clone(),
            });
        }
        match frame.dir.next_name() {
            None => {
                let finished = frames.pop();
                // Leaving a top-level folder (depth 1) completes its row.
                if frames.len() == 1 && finished.is_some() {
                    if let Some(open) = walker.current_row.take() {
                        walker.rows.finish_row(open.id, open.row);
                    }
                }
            }
            Some(Err(_)) => {
                // Listing failed part-way: count it and stop reading this folder.
                let row = frame.row;
                walker.mark_unreadable(row);
                let finished = frames.pop();
                if frames.len() == 1 && finished.is_some() {
                    if let Some(open) = walker.current_row.take() {
                        walker.rows.finish_row(open.id, open.row);
                    }
                }
                if frames.is_empty() {
                    return Err(ScanError::RootUnreadable);
                }
            }
            Some(Ok(name)) => {
                let depth = frames.len();
                let frame = frames.last().expect("a frame is on the stack");
                if let Some(child) = walker.visit(frame, &name, depth) {
                    frames.push(child);
                }
            }
        }
    }
    Ok(walker.finish(false))
}
