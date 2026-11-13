//! Read-only walk of one fixed location for the Storage page. Like the folder scanner, every
//! folder is opened relative to its open parent with no-follow semantics (`fsdir`), symlinks
//! are never followed and other volumes are not entered; no file is opened.
//!
//! Memory is bounded: the walk is depth-first and holds only the folders on the current path
//! (at most `max_depth`); each holds at most `2 × tree_children` kept subfolders; folders
//! deeper than `tree_depth` or smaller than `min_node_bytes` are summed, not kept; large files
//! and category items are bounded lists; hard-link tracking is capped at `hard_links`.
//!
//! Anything not counted (unreadable, too deep, or a hard link past the tracking limit) marks
//! its folder and category as minimums, so a hard link is never counted twice. (APFS clones
//! are still counted per copy; see docs/storage-discovery.md.)

use std::collections::HashSet;
use std::ffi::{CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::apps::AppIndex;
use super::classify::{lock, possible_below, rule, Category, Lock, RootKind, Scope};
use crate::storage::fsdir::{display_name, DirHandle, FileStat};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Folder levels below each location kept for drilling down.
    pub tree_depth: usize,
    /// Subfolders kept per folder; the rest are combined into one row.
    pub tree_children: usize,
    /// Smaller folders are combined into their parent's "other items" row.
    pub min_node_bytes: u64,
    /// Files at least this large are listed.
    pub large_file_bytes: u64,
    pub large_files: usize,
    /// Largest items listed per category.
    pub category_items: usize,
    /// Folders open at once along the current path, including the root.
    pub max_depth: usize,
    pub hard_links: usize,
    pub progress_interval: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            tree_depth: 4,
            tree_children: 12,
            min_node_bytes: 100_000_000,
            large_file_bytes: 1_000_000_000,
            large_files: 50,
            category_items: 3,
            max_depth: 128,
            hard_links: 1_000_000,
            progress_interval: Duration::from_millis(250),
        }
    }
}

/// One fixed location to walk.
#[derive(Debug, Clone)]
pub struct RootSpec {
    pub path: PathBuf,
    pub kind: RootKind,
    /// Row name in Largest folders.
    pub name: String,
    /// How paths below the root are shown, such as `~` or `/opt`.
    pub display_root: String,
    /// Paths below the root that are not walked or counted.
    pub excluded: &'static [&'static str],
}

impl RootSpec {
    fn label(&self, relative: &str) -> String {
        if relative.is_empty() {
            self.display_root.clone()
        } else {
            format!("{}/{relative}", self.display_root)
        }
    }

    /// Applications are listed by name alone; elsewhere the full location is clearer.
    fn item_label(&self, relative: &str) -> String {
        if self.kind == RootKind::Applications {
            relative.to_owned()
        } else {
            self.label(relative)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    Folder,
    /// Smaller folders and loose files combined into one row.
    Others,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    /// Empty for `others`. Invalid UTF-8 is escaped.
    pub name: String,
    pub kind: NodeKind,
    pub bytes: u64,
    pub category: Category,
    /// Something inside couldn't be read, so the size is a minimum.
    pub lower_bound: bool,
    /// `None` when the folder isn't broken down further.
    pub children: Option<Vec<Node>>,
}

impl Node {
    pub fn unreadable(name: String, category: Category) -> Self {
        Self {
            name,
            kind: NodeKind::Folder,
            bytes: 0,
            category,
            lower_bound: true,
            children: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub label: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFile {
    pub name: String,
    /// The folder it is in, shown as `~/Downloads` or `/Library/...`.
    pub folder: String,
    pub bytes: u64,
    /// Last modified, in milliseconds since the Unix epoch; `None` if not reported.
    pub modified_ms: Option<u64>,
    pub category: Category,
    pub lock: Option<Lock>,
    /// Adds it to Cleanup's review (inspect only) with `add_storage_file_to_review`.
    pub review_id: String,
    /// Where it is and what it was; kept in Rust (for Show in Finder), never sent.
    #[serde(skip)]
    pub source: FileSource,
}

/// A large file's exact path and identity when it was counted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileSource {
    pub path: PathBuf,
    pub stat: Option<FileStat>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    /// Entries or folders that couldn't be read; their size is unknown, not zero.
    pub unreadable: u64,
    pub symlinks_skipped: u64,
    pub other_volumes_skipped: u64,
    pub too_deep: u64,
    /// Hard-linked files seen after the tracking limit. They may duplicate files already
    /// counted, so they aren't counted.
    pub hard_links_skipped: u64,
}

/// Keeps `limit` entries, largest first; ties keep the earlier one.
fn keep_largest<T>(list: &mut Vec<T>, entry: T, limit: usize, size: impl Fn(&T) -> u64) {
    let position = list.partition_point(|kept| size(kept) >= size(&entry));
    if position < limit {
        list.insert(position, entry);
        list.truncate(limit);
    }
}

/// Totals and lists gathered across every location of one analysis.
pub struct Accumulator {
    pub limits: Limits,
    pub category_bytes: [u64; 6],
    /// Something that would have counted toward the category wasn't counted, so its total
    /// is a minimum.
    pub category_lower_bound: [bool; 6],
    pub category_items: [Vec<Item>; 6],
    pub large_files: Vec<LargeFile>,
    /// More files reached the size threshold than the list keeps.
    pub large_files_truncated: bool,
    pub coverage: Coverage,
    pub counted_bytes: u64,
    /// Installed apps, whose data folders count with Applications.
    pub apps: AppIndex,
    seen_links: HashSet<(u64, u64)>,
    last_progress: Instant,
}

impl Accumulator {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            category_bytes: [0; 6],
            category_lower_bound: [false; 6],
            category_items: Default::default(),
            large_files: Vec::new(),
            large_files_truncated: false,
            coverage: Coverage::default(),
            counted_bytes: 0,
            apps: AppIndex::default(),
            seen_links: HashSet::new(),
            last_progress: Instant::now(),
        }
    }

    fn incomplete(&mut self, category: Category) {
        self.category_lower_bound[category.index()] = true;
    }

    fn offer_item(&mut self, category: Category, label: String, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let limit = self.limits.category_items;
        keep_largest(
            &mut self.category_items[category.index()],
            Item { label, bytes },
            limit,
            |item| item.bytes,
        );
    }

    fn offer_large_file(&mut self, file: LargeFile) {
        if self.large_files.len() >= self.limits.large_files {
            self.large_files_truncated = true;
        }
        let limit = self.limits.large_files;
        keep_largest(&mut self.large_files, file, limit, |file| file.bytes);
    }
}

struct Frame {
    dir: DirHandle,
    name: String,
    relative: String,
    /// The exact path (`relative` is display text), for the large files found in it.
    path: PathBuf,
    depth: usize,
    category: Category,
    /// Each folder inside is listed as its own category item.
    children_are_items: bool,
    /// Set when this folder is a category item: bytes of its files in its category.
    item_bytes: Option<u64>,
    /// Position on the stack of the nearest folder that is a category item.
    item_frame: Option<usize>,
    bytes: u64,
    lower_bound: bool,
    children: Vec<Node>,
}

impl Frame {
    fn keep_child(&mut self, node: Node, limit: usize) {
        self.children.push(node);
        if self.children.len() > limit.saturating_mul(2) {
            sort_nodes(&mut self.children);
            self.children.truncate(limit);
        }
    }

    fn into_node(self, limit: usize) -> Node {
        let mut children = self.children;
        let children = (!children.is_empty()).then(|| {
            sort_nodes(&mut children);
            children.truncate(limit);
            let kept: u64 = children.iter().map(|child| child.bytes).sum();
            let rest = self.bytes.saturating_sub(kept);
            if rest > 0 {
                children.push(Node {
                    name: String::new(),
                    kind: NodeKind::Others,
                    bytes: rest,
                    category: self.category,
                    lower_bound: false,
                    children: None,
                });
            }
            children
        });
        Node {
            name: self.name,
            kind: NodeKind::Folder,
            bytes: self.bytes,
            category: self.category,
            lower_bound: self.lower_bound,
            children,
        }
    }
}

fn sort_nodes(nodes: &mut [Node]) {
    nodes.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

pub enum RootOutcome {
    /// The location doesn't exist on this Mac (for example `/opt` without Homebrew).
    Missing,
    Walked {
        node: Node,
        cancelled: bool,
    },
}

struct Walk<'a> {
    spec: &'a RootSpec,
    root_dev: u64,
    acc: &'a mut Accumulator,
}

impl Walk<'_> {
    /// Something in `frame` wasn't counted; its size, and those of `categories`, are minimums.
    fn not_counted(&mut self, frame: &mut Frame, categories: &[Category]) {
        frame.lower_bound = true;
        for &category in categories {
            self.acc.incomplete(category);
        }
    }

    /// The folder (or entry) at `relative` of `category` couldn't be read: everything its
    /// contents could have counted toward is a minimum.
    fn unreadable(&mut self, frame: &mut Frame, relative: &str, category: Category) {
        self.acc.coverage.unreadable += 1;
        let categories = possible_below(self.spec.kind, relative, category);
        self.not_counted(frame, &categories);
    }

    fn count_file(
        &mut self,
        frames: &mut [Frame],
        relative: &str,
        name: &OsStr,
        shown: String,
        stat: &FileStat,
    ) {
        let Some(top) = frames.len().checked_sub(1) else {
            return;
        };
        if stat.nlink > 1 {
            let identity = (stat.dev, stat.ino);
            if self.acc.seen_links.contains(&identity) {
                // Another hard link to a file already counted.
                return;
            }
            if self.acc.seen_links.len() >= self.acc.limits.hard_links {
                // Past the tracking limit it may be a file already counted, so it isn't
                // counted at all: totals stay minimums rather than possibly inflated.
                self.acc.coverage.hard_links_skipped += 1;
                let category = frames[top].category;
                self.not_counted(&mut frames[top], &[category]);
                return;
            }
            self.acc.seen_links.insert(identity);
        }
        let bytes = stat.blocks.saturating_mul(512);
        let (category, item_frame) = {
            let frame = &mut frames[top];
            frame.bytes = frame.bytes.saturating_add(bytes);
            (frame.category, frame.item_frame)
        };
        if let Some(item) = item_frame.and_then(|index| frames.get_mut(index)) {
            item.item_bytes = Some(item.item_bytes.unwrap_or(0).saturating_add(bytes));
        }
        let index = category.index();
        self.acc.category_bytes[index] = self.acc.category_bytes[index].saturating_add(bytes);
        self.acc.counted_bytes = self.acc.counted_bytes.saturating_add(bytes);
        if bytes >= self.acc.limits.large_file_bytes {
            let modified_ms = u64::try_from(stat.modified_s)
                .ok()
                .filter(|&seconds| seconds > 0)
                .and_then(|seconds| seconds.checked_mul(1000));
            self.acc.offer_large_file(LargeFile {
                name: shown,
                folder: self.spec.label(&frames[top].relative),
                bytes,
                modified_ms,
                category,
                lock: lock(self.spec.kind, relative, category),
                review_id: crate::ids::new_id(0),
                source: FileSource {
                    path: frames[top].path.join(name),
                    stat: Some(*stat),
                },
            });
        }
    }

    /// Visits one entry of the top frame. Returns a child folder to descend into, if any.
    fn visit(&mut self, frames: &mut [Frame], name: &CStr) -> Option<Frame> {
        let top = frames.len().checked_sub(1)?;
        let os_name = OsStr::from_bytes(name.to_bytes());
        let shown = display_name(os_name);
        let relative = join(&frames[top].relative, &shown);
        if self.spec.excluded.contains(&relative.as_str()) {
            return None;
        }
        let Ok(stat) = frames[top].dir.stat_child(name) else {
            // It may be a folder, so it counts as one with the category its location gives.
            let category = rule(self.spec.kind, &relative, &shown, &self.acc.apps)
                .map_or(frames[top].category, |(category, _)| category);
            self.unreadable(&mut frames[top], &relative, category);
            return None;
        };
        if stat.is_symlink() {
            self.acc.coverage.symlinks_skipped += 1;
            return None;
        }
        if !stat.is_dir() {
            // Regular files and other non-directory entries (sockets, FIFOs) use disk space too.
            self.count_file(frames, &relative, os_name, shown, &stat);
            return None;
        }
        if stat.dev != self.root_dev {
            self.acc.coverage.other_volumes_skipped += 1;
            return None;
        }
        // Classified before opening, so a folder that can't be read still marks the categories
        // it would have counted toward.
        let parent = &frames[top];
        let own = frames.len();
        let named = rule(self.spec.kind, &relative, &shown, &self.acc.apps)
            .filter(|&(category, _)| !(category == parent.category && parent.item_frame.is_some()));
        let (category, is_item, children_are_items) = match named {
            Some((category, Scope::Itself)) => (category, true, false),
            Some((category, Scope::Children)) => (category, false, true),
            None => (parent.category, parent.children_are_items, false),
        };
        let item_frame = if is_item {
            Some(own)
        } else if named.is_some() {
            None
        } else {
            parent.item_frame
        };
        let depth = parent.depth + 1;
        if frames.len() >= self.acc.limits.max_depth {
            self.acc.coverage.too_deep += 1;
            let categories = possible_below(self.spec.kind, &relative, category);
            self.not_counted(&mut frames[top], &categories);
            return None;
        }
        // Open relative to the parent with no-follow, then confirm it is the folder just
        // checked: a swap in between is refused or detected, never followed.
        let opened = frames[top].dir.open_child(name).ok().filter(|(_, opened)| {
            opened.is_dir() && opened.dev == stat.dev && opened.ino == stat.ino
        });
        let Some((dir, _)) = opened else {
            self.unreadable(&mut frames[top], &relative, category);
            return None;
        };
        Some(Frame {
            dir,
            name: shown,
            relative,
            path: frames[top].path.join(os_name),
            depth,
            category,
            children_are_items,
            item_bytes: is_item.then_some(0),
            item_frame,
            bytes: 0,
            lower_bound: false,
            children: Vec::new(),
        })
    }

    /// Finishes the top frame, folding it into its parent. Returns the root's node.
    fn finish_frame(&mut self, frames: &mut Vec<Frame>) -> Option<Node> {
        let mut frame = frames.pop()?;
        if let Some(bytes) = frame.item_bytes.take() {
            let label = self.spec.item_label(&frame.relative);
            self.acc.offer_item(frame.category, label, bytes);
        }
        let depth = frame.depth;
        let limits = self.acc.limits;
        let node = frame.into_node(limits.tree_children);
        let Some(parent) = frames.last_mut() else {
            return Some(node);
        };
        parent.bytes = parent.bytes.saturating_add(node.bytes);
        parent.lower_bound |= node.lower_bound;
        if depth <= limits.tree_depth && node.bytes >= limits.min_node_bytes.max(1) {
            parent.keep_child(node, limits.tree_children);
        }
        None
    }
}

/// Walks one location read-only, adding to `acc`. `progress` receives the bytes counted so
/// far across the whole analysis, at most every `limits.progress_interval`.
pub fn walk_root(
    spec: &RootSpec,
    acc: &mut Accumulator,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64),
) -> RootOutcome {
    let category = spec.kind.base();
    let (root_dir, stat) = match DirHandle::open_root(&spec.path) {
        Ok(opened) => opened,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return RootOutcome::Missing,
        Err(_) => {
            acc.coverage.unreadable += 1;
            for below in possible_below(spec.kind, "", category) {
                acc.incomplete(below);
            }
            return RootOutcome::Walked {
                node: Node::unreadable(spec.name.clone(), category),
                cancelled: false,
            };
        }
    };
    let mut walk = Walk {
        spec,
        root_dev: stat.dev,
        acc,
    };
    let mut frames = vec![Frame {
        dir: root_dir,
        name: spec.name.clone(),
        relative: String::new(),
        path: spec.path.clone(),
        depth: 0,
        category,
        children_are_items: true,
        item_bytes: None,
        item_frame: None,
        bytes: 0,
        lower_bound: false,
        children: Vec::new(),
    }];

    let mut cancelled = false;
    loop {
        if !cancelled && cancel.load(Ordering::Relaxed) {
            // Stop reading; unwind the open folders so the partial totals stay consistent.
            cancelled = true;
        }
        if walk.acc.last_progress.elapsed() >= walk.acc.limits.progress_interval {
            walk.acc.last_progress = Instant::now();
            progress(walk.acc.counted_bytes);
        }
        let next = if cancelled {
            None
        } else {
            frames.last_mut().and_then(|frame| frame.dir.next_name())
        };
        match next {
            None => {
                if let Some(node) = walk.finish_frame(&mut frames) {
                    return RootOutcome::Walked { node, cancelled };
                }
            }
            Some(Err(_)) => {
                // Listing failed part-way: count it and stop reading this folder.
                if let Some(frame) = frames.last_mut() {
                    let (relative, category) = (frame.relative.clone(), frame.category);
                    walk.unreadable(frame, &relative, category);
                }
                if let Some(node) = walk.finish_frame(&mut frames) {
                    return RootOutcome::Walked { node, cancelled };
                }
            }
            Some(Ok(name)) => {
                if let Some(child) = walk.visit(&mut frames, &name) {
                    frames.push(child);
                }
            }
        }
    }
}
