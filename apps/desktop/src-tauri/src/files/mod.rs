//! The Files page: tools that find your own files worth a look (ADR-019). Large & Old, exact
//! Duplicates, Similar Images, Downloads by the app that downloaded them, and AI clutter
//! (model downloads, AI apps' caches and logs).
//!
//! Read-only. The only file contents read are those of same-size files, to tell copies apart
//! (SHA-256, never stored), and images' pixels, decoded by macOS to a tiny thumbnail, to tell
//! similar ones apart. A download's source app comes from the `com.apple.quarantine` tag macOS
//! puts on downloads; the address it came from is never read. Everything found can be sent to
//! Cleanup's review as something you chose (`Kind::UserChosen`), moving only into quarantine
//! when you approve it. In a group of copies one is always kept.

pub mod images;
pub mod walk;

use std::collections::HashMap;
use std::ffi::CString;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cleanup::policy::{self, Kind};
use crate::developer::{measure_dir, open_absolute, Links, Measured};
use crate::ids::new_id;
use crate::storage::category::{category_of, FileCategory};
use crate::storage::explorer::Chosen;
use crate::storage::fsdir::FileStat;
use crate::system_monitor::display_path;

const DAY_S: i64 = 86_400;
/// Files listed per group, largest first.
const MAX_GROUP_ITEMS: usize = 500;
/// Groups of copies or similar images listed, most space first.
const MAX_GROUPS: usize = 400;
/// Copies smaller than this aren't worth a look.
const MIN_COPY_BYTES: u64 = 1_000_000;
/// Old files smaller than this aren't listed.
const MIN_OLD_BYTES: u64 = 50_000_000;
/// The first bytes hashed to rule copies out cheaply.
const HEAD_BYTES: usize = 64 * 1024;
/// A model download unused this long is "not used lately".
const MODEL_IDLE_DAYS: u64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    LargeOld,
    Duplicates,
    Similar,
    Downloads,
    Ai,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    pub large_min_mb: u32,
    pub old_days: u32,
}

impl Options {
    pub fn valid(&self) -> bool {
        [100, 500, 1_000, 5_000].contains(&self.large_min_mb)
            && [90, 180, 365, 730].contains(&self.old_days)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupKind {
    Large,
    Old,
    /// Identical copies: one is always kept.
    Copies,
    /// Images that look alike: one is always kept.
    Similar,
    /// Downloads by the app that downloaded them.
    Source,
    /// AI models, caches or logs of one tool.
    Ai,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileItem {
    pub id: String,
    pub name: String,
    /// The folder it is in, shown as `~/…`.
    pub location: String,
    pub bytes: u64,
    pub lower_bound: bool,
    pub folder: bool,
    pub modified_ms: Option<u64>,
    /// When it was last opened, as macOS records it (lazily).
    pub opened_ms: Option<u64>,
    pub file_category: Option<FileCategory>,
    /// It can be sent to the review (where Kleari may move what you choose).
    pub selectable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: String,
    pub kind: GroupKind,
    /// A source app, an AI tool, or empty when the kind says it all.
    pub label: String,
    /// What it holds, for AI groups (models, cache, logs).
    pub detail: Option<String>,
    pub bytes: u64,
    /// For copies and similar images: what removing all but one would free.
    pub reclaimable: u64,
    pub items: Vec<FileItem>,
    /// Items beyond `MAX_GROUP_ITEMS`.
    pub more: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanStatus {
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesScan {
    pub status: ScanStatus,
    pub tool: Tool,
    pub groups: Vec<Group>,
    pub files_seen: u64,
    pub unreadable: u64,
    pub truncated: bool,
}

/// One found item, kept in Rust: what the review needs, and its group when one must be kept.
#[derive(Debug, Clone)]
struct Found {
    chosen: Chosen,
    /// Its group, when one member must be kept, and whether it is the one kept by default.
    keep_group: Option<(String, bool)>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Default)]
pub struct FilesState {
    active: Mutex<Option<Arc<AtomicBool>>>,
    found: Mutex<HashMap<String, Found>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScanBusy;

impl FilesState {
    pub fn begin(&self) -> Result<Arc<AtomicBool>, ScanBusy> {
        let mut active = lock(&self.active);
        if active.is_some() {
            return Err(ScanBusy);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(Arc::clone(&cancel));
        Ok(cancel)
    }

    pub fn cancel(&self) {
        if let Some(cancel) = &*lock(&self.active) {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Ends a scan that couldn't start, keeping what the last one found.
    pub fn abandon(&self) {
        *lock(&self.active) = None;
    }

    /// Ends the scan; a completed one replaces what the last scan of the same items found.
    fn finish(&self, found: Option<HashMap<String, Found>>) {
        if let Some(found) = found {
            let mut kept = lock(&self.found);
            kept.extend(found);
            // Bounded: keep the newest scan's items when it grows too large.
            if kept.len() > 200_000 {
                kept.clear();
            }
        }
        *lock(&self.active) = None;
    }

    /// What `ids` stand for, ready for the review. In a group where one copy must be kept, if
    /// every member was asked for, the group's first (the one the page marks Keep) is held back.
    pub fn chosen(&self, ids: &[String]) -> Vec<Chosen> {
        let found = lock(&self.found);
        let asked: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
        let mut groups: HashMap<&str, Vec<(&str, bool)>> = HashMap::new();
        for (id, item) in found.iter() {
            if let Some((group, keeper)) = &item.keep_group {
                groups
                    .entry(group.as_str())
                    .or_default()
                    .push((id.as_str(), *keeper));
            }
        }
        let mut held = std::collections::HashSet::new();
        for members in groups.values() {
            if members.iter().all(|(member, _)| asked.contains(member)) {
                if let Some((keeper, _)) = members.iter().find(|(_, keeper)| *keeper) {
                    held.insert(*keeper);
                }
            }
        }
        ids.iter()
            .filter(|id| !held.contains(id.as_str()))
            .filter_map(|id| found.get(id))
            .map(|item| item.chosen.clone())
            .collect()
    }
}

/// Runs `tool`, read-only.
pub fn scan(
    state: &FilesState,
    tool: Tool,
    options: Options,
    home: &Path,
    excluded: &[PathBuf],
    now_ms: u64,
    cancel: &AtomicBool,
) -> FilesScan {
    let mut found = HashMap::new();
    let mut result = FilesScan {
        status: ScanStatus::Completed,
        tool,
        groups: Vec::new(),
        files_seen: 0,
        unreadable: 0,
        truncated: false,
    };
    let mut ctx = Ctx {
        home,
        now_ms,
        found: &mut found,
    };
    match tool {
        Tool::LargeOld => large_old(&mut ctx, options, excluded, cancel, &mut result),
        Tool::Duplicates => duplicates(&mut ctx, excluded, cancel, &mut result),
        Tool::Similar => images::similar(&mut ctx, excluded, cancel, &mut result),
        Tool::Downloads => downloads(&mut ctx, excluded, cancel, &mut result),
        Tool::Ai => ai(&mut ctx, excluded, cancel, &mut result),
    }
    if cancel.load(Ordering::Relaxed) {
        result.status = ScanStatus::Cancelled;
    }
    state.finish((result.status == ScanStatus::Completed).then_some(found));
    result
}

/// What every tool needs to record what it found.
pub(crate) struct Ctx<'a> {
    home: &'a Path,
    now_ms: u64,
    found: &'a mut HashMap<String, Found>,
}

impl Ctx<'_> {
    /// Records a file or folder and returns the page's view of it.
    pub(crate) fn item(
        &mut self,
        path: &Path,
        stat: &FileStat,
        measured: Measured,
        keep_group: Option<(&str, bool)>,
    ) -> FileItem {
        let id = new_id(self.now_ms);
        let selectable =
            policy::within_bounds(&Kind::UserChosen, path, self.home, None, |_| Vec::new());
        let folder = stat.is_dir();
        let item = FileItem {
            id: id.clone(),
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            location: path
                .parent()
                .map(|parent| display_path(parent, Some(self.home)))
                .unwrap_or_default(),
            bytes: measured.bytes,
            lower_bound: measured.lower_bound,
            folder,
            modified_ms: ms(measured.newest_modified_s.max(stat.modified_s)),
            opened_ms: (!folder).then(|| ms(stat.accessed_s)).flatten(),
            file_category: (!folder).then(|| category_of(path)),
            selectable,
        };
        if selectable {
            self.found.insert(
                id,
                Found {
                    chosen: Chosen {
                        name: item.name.clone(),
                        path: path.to_path_buf(),
                        stat: *stat,
                        measured,
                    },
                    keep_group: keep_group.map(|(group, keeper)| (group.to_owned(), keeper)),
                },
            );
        }
        item
    }

    fn days_since(&self, seconds: i64) -> u64 {
        let now_s = i64::try_from(self.now_ms / 1000).unwrap_or(i64::MAX);
        u64::try_from(now_s.saturating_sub(seconds) / DAY_S).unwrap_or(0)
    }
}

fn ms(seconds: i64) -> Option<u64> {
    u64::try_from(seconds)
        .ok()
        .filter(|&seconds| seconds > 0)
        .and_then(|seconds| seconds.checked_mul(1000))
}

pub(crate) fn file_measured(stat: &FileStat) -> Measured {
    Measured {
        bytes: stat.blocks.saturating_mul(512),
        files: 1,
        lower_bound: false,
        parts: Vec::new(),
        newest_modified_s: stat.modified_s,
    }
}

pub(crate) fn group(
    kind: GroupKind,
    label: String,
    detail: Option<String>,
    mut items: Vec<FileItem>,
    reclaimable: u64,
) -> Group {
    items.sort_by_key(|item| std::cmp::Reverse(item.bytes));
    let bytes = items
        .iter()
        .fold(0_u64, |total, item| total.saturating_add(item.bytes));
    let more = u64::try_from(items.len().saturating_sub(MAX_GROUP_ITEMS)).unwrap_or(0);
    items.truncate(MAX_GROUP_ITEMS);
    Group {
        id: new_id(0),
        kind,
        label,
        detail,
        bytes,
        reclaimable,
        items,
        more,
    }
}

fn record_walk(result: &mut FilesScan, walked: walk::Walked) {
    result.files_seen = result.files_seen.saturating_add(walked.files);
    result.unreadable = result.unreadable.saturating_add(walked.unreadable);
    result.truncated |= walked.truncated;
}

/// Files of at least `large_min_mb`, and files of 50 MB or more not opened or changed for
/// `old_days`.
fn large_old(
    ctx: &mut Ctx<'_>,
    options: Options,
    excluded: &[PathBuf],
    cancel: &AtomicBool,
    result: &mut FilesScan,
) {
    let large_min = u64::from(options.large_min_mb).saturating_mul(1_000_000);
    let mut large: Vec<(PathBuf, FileStat)> = Vec::new();
    let mut old: Vec<(PathBuf, FileStat)> = Vec::new();
    let home = ctx.home.to_path_buf();
    let walked = walk::walk(&home, excluded, cancel, &mut |path, stat| {
        let bytes = stat.blocks.saturating_mul(512);
        if bytes >= large_min {
            large.push((path.to_path_buf(), *stat));
        } else if bytes >= MIN_OLD_BYTES
            && ctx.days_since(stat.accessed_s.max(stat.modified_s)) >= u64::from(options.old_days)
        {
            old.push((path.to_path_buf(), *stat));
        }
    });
    record_walk(result, walked);
    for (kind, list) in [(GroupKind::Large, large), (GroupKind::Old, old)] {
        let items = list
            .iter()
            .map(|(path, stat)| ctx.item(path, stat, file_measured(stat), None))
            .collect();
        result
            .groups
            .push(group(kind, String::new(), None, items, 0));
    }
}

/// SHA-256 of the file's first `limit` bytes (all of it when `None`), if it is still the file
/// the walk saw.
fn hash(
    path: &Path,
    stat: &FileStat,
    limit: Option<usize>,
    cancel: &AtomicBool,
) -> Option<[u8; 32]> {
    let mut file = walk::open_same(path, stat)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    let mut left = limit.unwrap_or(usize::MAX);
    while left > 0 {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let want = buffer.len().min(left);
        let read = file.read(&mut buffer[..want]).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        left -= read;
    }
    Some(hasher.finalize().into())
}

/// Files with identical contents: same size, then the same first 64 KB, then the same SHA-256
/// of everything. Hard links to one file aren't copies.
fn duplicates(
    ctx: &mut Ctx<'_>,
    excluded: &[PathBuf],
    cancel: &AtomicBool,
    result: &mut FilesScan,
) {
    let mut by_size: HashMap<u64, Vec<(PathBuf, FileStat)>> = HashMap::new();
    let mut seen = std::collections::HashSet::new();
    let home = ctx.home.to_path_buf();
    let walked = walk::walk(&home, excluded, cancel, &mut |path, stat| {
        // `st_size` isn't kept by FileStat; allocated blocks are close enough to group by, and
        // the hashes decide.
        let bytes = stat.blocks.saturating_mul(512);
        if bytes >= MIN_COPY_BYTES && seen.insert((stat.dev, stat.ino)) {
            by_size
                .entry(bytes)
                .or_default()
                .push((path.to_path_buf(), *stat));
        }
    });
    record_walk(result, walked);
    let mut copies: Vec<Vec<(PathBuf, FileStat)>> = Vec::new();
    for (_, same_size) in by_size.into_iter().filter(|(_, list)| list.len() > 1) {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let mut by_head: HashMap<[u8; 32], Vec<(PathBuf, FileStat)>> = HashMap::new();
        for (path, stat) in same_size {
            if let Some(head) = hash(&path, &stat, Some(HEAD_BYTES), cancel) {
                by_head.entry(head).or_default().push((path, stat));
            }
        }
        for (_, same_head) in by_head.into_iter().filter(|(_, list)| list.len() > 1) {
            let mut by_full: HashMap<[u8; 32], Vec<(PathBuf, FileStat)>> = HashMap::new();
            for (path, stat) in same_head {
                if let Some(full) = hash(&path, &stat, None, cancel) {
                    by_full.entry(full).or_default().push((path, stat));
                }
            }
            copies.extend(by_full.into_values().filter(|list| list.len() > 1));
        }
    }
    let wasted = |list: &Vec<(PathBuf, FileStat)>| {
        list.first()
            .map_or(0, |(_, stat)| stat.blocks.saturating_mul(512))
            .saturating_mul(u64::try_from(list.len().saturating_sub(1)).unwrap_or(0))
    };
    copies.sort_by_key(|list| std::cmp::Reverse(wasted(list)));
    copies.truncate(MAX_GROUPS);
    for mut list in copies {
        // The copy kept by default: the one changed longest ago (likely the original), first.
        list.sort_by_key(|(path, stat)| (stat.modified_s, path.clone()));
        let reclaimable = wasted(&list);
        let group_id = new_id(0);
        let items: Vec<FileItem> = list
            .iter()
            .enumerate()
            .map(|(index, (path, stat))| {
                ctx.item(
                    path,
                    stat,
                    file_measured(stat),
                    Some((&group_id, index == 0)),
                )
            })
            .collect();
        let mut made = group(
            GroupKind::Copies,
            String::new(),
            None,
            Vec::new(),
            reclaimable,
        );
        made.id = group_id;
        made.bytes = items
            .iter()
            .fold(0_u64, |total, item| total.saturating_add(item.bytes));
        made.items = items;
        result.groups.push(made);
    }
}

/// The app a download came from, from the `com.apple.quarantine` tag macOS adds
/// (`flags;time;agent;id`). Only the agent's name is read.
pub fn download_agent(path: &Path) -> Option<String> {
    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let name = c"com.apple.quarantine";
    let mut buffer = [0_u8; 1024];
    // SAFETY: `path` and `name` are NUL-terminated; `buffer` is writable for its length.
    #[cfg(target_os = "macos")]
    let read = unsafe {
        libc::getxattr(
            path.as_ptr(),
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            0,
            libc::XATTR_NOFOLLOW,
        )
    };
    #[cfg(not(target_os = "macos"))]
    let read: isize = {
        let _ = (&path, name, &mut buffer);
        -1
    };
    let read = usize::try_from(read).ok()?;
    let text = std::str::from_utf8(&buffer[..read.min(buffer.len())]).ok()?;
    let agent = text.split(';').nth(2)?.trim();
    (!agent.is_empty() && agent.len() <= 128 && !agent.contains('/')).then(|| agent.to_owned())
}

/// What is directly in Downloads, by the app that downloaded it.
fn downloads(ctx: &mut Ctx<'_>, excluded: &[PathBuf], cancel: &AtomicBool, result: &mut FilesScan) {
    let folder = ctx.home.join("Downloads");
    if crate::preferences::touches_excluded(&folder, excluded) {
        return;
    }
    let Some((mut dir, dev)) = open_absolute(&folder) else {
        result.unreadable += 1;
        return;
    };
    let mut links = Links::default();
    let mut by_agent: HashMap<String, Vec<FileItem>> = HashMap::new();
    while let Some(Ok(name)) = dir.next_name() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        if name.to_bytes().starts_with(b".") {
            continue;
        }
        let Ok(stat) = dir.stat_child(&name) else {
            continue;
        };
        if stat.is_symlink() || stat.dev != dev || !(stat.is_file() || stat.is_dir()) {
            continue;
        }
        let path = folder.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
        if crate::preferences::touches_excluded(&path, excluded) {
            continue;
        }
        let measured = if stat.is_dir() {
            match dir.open_child(&name) {
                Ok((child, opened)) if opened.ino == stat.ino => {
                    measure_dir(child, dev, &mut links, cancel)
                }
                _ => continue,
            }
        } else {
            file_measured(&stat)
        };
        result.files_seen += 1;
        let agent = download_agent(&path).unwrap_or_else(|| "Unknown".to_owned());
        let item = ctx.item(&path, &stat, measured, None);
        by_agent.entry(agent).or_default().push(item);
    }
    let mut groups: Vec<Group> = by_agent
        .into_iter()
        .map(|(agent, items)| group(GroupKind::Source, agent, None, items, 0))
        .collect();
    groups.sort_by_key(|group| std::cmp::Reverse(group.bytes));
    result.groups.extend(groups);
}

/// Where AI tools keep downloads, caches and logs, relative to the home folder: the label, what
/// it holds, and whether its entries are listed one by one (each model a row) or as a whole.
struct AiPlace {
    label: &'static str,
    detail: &'static str,
    relative: &'static str,
    /// List what is inside (each model) rather than the folder as one item.
    per_entry: bool,
}

const AI_PLACES: &[AiPlace] = &[
    AiPlace {
        label: "Ollama",
        detail: "Models. Remove one model with `ollama rm`; this is all of them",
        relative: ".ollama/models",
        per_entry: false,
    },
    AiPlace {
        label: "Ollama",
        detail: "Logs",
        relative: ".ollama/logs",
        per_entry: false,
    },
    AiPlace {
        label: "Hugging Face",
        detail: "Downloaded models and datasets",
        relative: ".cache/huggingface/hub",
        per_entry: true,
    },
    AiPlace {
        label: "LM Studio",
        detail: "Downloaded models",
        relative: ".lmstudio/models",
        per_entry: true,
    },
    AiPlace {
        label: "LM Studio",
        detail: "Downloaded models",
        relative: ".cache/lm-studio/models",
        per_entry: true,
    },
    AiPlace {
        label: "GPT4All",
        detail: "Downloaded models",
        relative: ".cache/gpt4all",
        per_entry: true,
    },
    AiPlace {
        label: "Whisper",
        detail: "Speech models",
        relative: ".cache/whisper",
        per_entry: true,
    },
    AiPlace {
        label: "PyTorch",
        detail: "Hub downloads",
        relative: ".cache/torch/hub",
        per_entry: true,
    },
    AiPlace {
        label: "Keras",
        detail: "Downloaded models",
        relative: ".keras/models",
        per_entry: true,
    },
    AiPlace {
        label: "ChatGPT",
        detail: "Cache",
        relative: "Library/Caches/com.openai.chat",
        per_entry: false,
    },
    AiPlace {
        label: "Claude",
        detail: "Cache",
        relative: "Library/Caches/com.anthropic.claudefordesktop",
        per_entry: false,
    },
    AiPlace {
        label: "Claude",
        detail: "Logs",
        relative: "Library/Logs/Claude",
        per_entry: false,
    },
    AiPlace {
        label: "Cursor",
        detail: "Cache",
        relative: "Library/Caches/com.todesktop.230313mzl4w4u92",
        per_entry: false,
    },
    AiPlace {
        label: "Copilot",
        detail: "Cache",
        relative: "Library/Caches/com.microsoft.copilot-mac",
        per_entry: false,
    },
];

/// AI models, caches and logs in known places. A model not used for 60 days is noted.
fn ai(ctx: &mut Ctx<'_>, excluded: &[PathBuf], cancel: &AtomicBool, result: &mut FilesScan) {
    let mut links = Links::default();
    for place in AI_PLACES {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let path = ctx.home.join(place.relative);
        if crate::preferences::touches_excluded(&path, excluded) {
            continue;
        }
        let Some(parent) = path.parent() else {
            continue;
        };
        let Some((parent_dir, dev)) = open_absolute(parent) else {
            continue;
        };
        let Some(name) = path
            .file_name()
            .and_then(|name| CString::new(name.as_bytes()).ok())
        else {
            continue;
        };
        let Ok(stat) = parent_dir.stat_child(&name) else {
            continue;
        };
        if !stat.is_dir() || stat.dev != dev {
            continue;
        }
        let mut items = Vec::new();
        if place.per_entry {
            let Ok((mut dir, _)) = parent_dir.open_child(&name) else {
                continue;
            };
            while let Some(Ok(entry)) = dir.next_name() {
                if entry.to_bytes().starts_with(b".") {
                    continue;
                }
                let Ok(child_stat) = dir.stat_child(&entry) else {
                    continue;
                };
                if child_stat.is_symlink() || child_stat.dev != dev {
                    continue;
                }
                let child = path.join(std::ffi::OsStr::from_bytes(entry.to_bytes()));
                let measured = if child_stat.is_dir() {
                    match dir.open_child(&entry) {
                        Ok((opened, opened_stat)) if opened_stat.ino == child_stat.ino => {
                            measure_dir(opened, dev, &mut links, cancel)
                        }
                        _ => continue,
                    }
                } else {
                    file_measured(&child_stat)
                };
                if measured.bytes == 0 {
                    continue;
                }
                items.push(ctx.item(&child, &child_stat, measured, None));
            }
        } else if let Ok((opened, opened_stat)) = parent_dir.open_child(&name) {
            if opened_stat.ino != stat.ino {
                continue;
            }
            let measured = measure_dir(opened, dev, &mut links, cancel);
            if measured.bytes > 0 {
                items.push(ctx.item(&path, &stat, measured, None));
            }
        }
        if items.is_empty() {
            continue;
        }
        let idle = items
            .iter()
            .filter(|item| {
                item.modified_ms.is_some_and(|ms| {
                    ctx.days_since(i64::try_from(ms / 1000).unwrap_or(0)) >= MODEL_IDLE_DAYS
                })
            })
            .count();
        let detail = if place.per_entry && idle > 0 {
            format!(
                "{} · {idle} not changed in {MODEL_IDLE_DAYS} days",
                place.detail
            )
        } else {
            place.detail.to_owned()
        };
        result.files_seen += u64::try_from(items.len()).unwrap_or(0);
        result.groups.push(group(
            GroupKind::Ai,
            place.label.to_owned(),
            Some(detail),
            items,
            0,
        ));
    }
    result
        .groups
        .sort_by_key(|group| std::cmp::Reverse(group.bytes));
}

#[cfg(test)]
mod tests;
