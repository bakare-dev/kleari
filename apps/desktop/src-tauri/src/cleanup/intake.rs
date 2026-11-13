//! Items sent to Review from the Developer and Storage pages. They are classified by the same
//! policy as the scan's findings; the page that sent them decides nothing.

use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use super::policy::Kind;
use super::{assess_now, location_of, new_id, Candidate, Source, StoredFacts};
use crate::developer::{open_absolute, ReviewItem, ReviewKind};
use crate::quarantine::Identity;
use crate::storage::analysis::LargeFile;
use crate::system_monitor::running_apps::RunningBundle;

/// A project with a commit within this many days is active.
const ACTIVE_DAYS: u64 = 14;
const DAY_MS: u64 = 86_400_000;
/// Storage lists files from this size.
const STORAGE_LARGE_BYTES: u64 = 1_000_000_000;

pub(crate) const LOCKFILES: &[&std::ffi::CStr] = &[
    c"package-lock.json",
    c"npm-shrinkwrap.json",
    c"pnpm-lock.yaml",
    c"yarn.lock",
    c"bun.lockb",
    c"bun.lock",
];

/// Folders above a project searched for a workspace lockfile.
const MAX_LOCKFILE_LEVELS: usize = 8;

/// Workspace manifests are read only up to this size.
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

/// Whether a lockfile (a regular file, not a symlink) can reinstall the project's packages:
/// next to its `node_modules`, or at the root of a workspace (pnpm, Yarn, npm or Bun) that
/// includes the project. An ancestor's lockfile counts only if that ancestor declares
/// workspaces (`pnpm-workspace.yaml`, or `workspaces` in `package.json`) whose patterns match
/// the project's path; any other lockfile above belongs to a different project and ends the
/// search. The search also ends at the repository root (a folder holding `.git`), `stop` (the
/// home folder) or 8 levels. Each folder is opened from `/` without following symlinks.
///
/// Folders excluded in Settings end the search unread, and an excluded entry (a lockfile,
/// `.git`) is never looked at; without what it would have shown, no lockfile is assumed.
pub(crate) fn has_lockfile(project: &Path, stop: &Path, excluded: &[PathBuf]) -> bool {
    use crate::preferences::{inside_excluded, touches_excluded};
    let entry = |folder: &Path, name: &std::ffi::CStr| {
        folder.join(std::ffi::OsStr::from_bytes(name.to_bytes()))
    };
    for folder in project.ancestors().take(MAX_LOCKFILE_LEVELS) {
        if inside_excluded(folder, excluded) {
            return false;
        }
        let Some((dir, _)) = open_absolute(folder) else {
            return false;
        };
        let lockfile = LOCKFILES.iter().any(|name| {
            !touches_excluded(&entry(folder, name), excluded)
                && dir.stat_child(name).is_ok_and(|stat| stat.is_file())
        });
        if lockfile {
            if folder == project {
                return true;
            }
            let Ok(relative) = project.strip_prefix(folder) else {
                return false;
            };
            // The workspace manifests are read only if neither is excluded.
            let manifests = [c"pnpm-workspace.yaml", c"package.json"];
            if manifests
                .iter()
                .any(|name| touches_excluded(&entry(folder, name), excluded))
            {
                return false;
            }
            return workspace_includes(&dir, relative);
        }
        // An excluded `.git` isn't looked at; the search stops there as at a repository root.
        let repository_root =
            touches_excluded(&entry(folder, c".git"), excluded) || dir.stat_child(c".git").is_ok();
        if repository_root || folder == stop || !folder.starts_with(stop) {
            return false;
        }
    }
    false
}

/// A small regular file in `dir`, opened without following a symlink, as text.
fn read_manifest(dir: &crate::storage::fsdir::DirHandle, name: &std::ffi::CStr) -> Option<String> {
    use std::io::Read;
    let file = dir.open_file_child(name).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return None;
    }
    let mut text = String::new();
    file.take(MAX_MANIFEST_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// The workspace patterns `dir` declares: `packages:` in `pnpm-workspace.yaml`, and
/// `workspaces` (a list, or `{ "packages": [...] }`) in `package.json`.
fn workspace_patterns(dir: &crate::storage::fsdir::DirHandle) -> Vec<String> {
    let mut patterns = Vec::new();
    if let Some(yaml) = read_manifest(dir, c"pnpm-workspace.yaml") {
        patterns.extend(pnpm_packages(&yaml));
    }
    if let Some(json) = read_manifest(dir, c"package.json") {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
            let workspaces = value.get("workspaces");
            let list = workspaces.and_then(|workspaces| {
                workspaces
                    .as_array()
                    .or_else(|| workspaces.get("packages")?.as_array())
            });
            patterns.extend(
                list.into_iter()
                    .flatten()
                    .filter_map(|entry| entry.as_str().map(str::to_owned)),
            );
        }
    }
    patterns
}

/// The entries of the top-level `packages:` list in a `pnpm-workspace.yaml`. Only that plain
/// list form is read; anything else gives no patterns (so nothing becomes eligible).
pub(crate) fn pnpm_packages(yaml: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    let mut in_packages = false;
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with(char::is_whitespace) && !trimmed.starts_with('-') {
            in_packages = trimmed == "packages:";
            continue;
        }
        if in_packages {
            if let Some(entry) = trimmed.strip_prefix('-') {
                let entry = entry.trim().trim_matches(|c| c == '\'' || c == '"');
                if !entry.is_empty() {
                    patterns.push(entry.to_owned());
                }
            }
        }
    }
    patterns
}

/// Whether the workspace rooted at `dir` includes the project at `relative` (a path below
/// it): some pattern matches and no `!` pattern excludes it.
fn workspace_includes(dir: &crate::storage::fsdir::DirHandle, relative: &Path) -> bool {
    let Some(relative) = relative.to_str() else {
        return false;
    };
    workspace_member(&workspace_patterns(dir), relative)
}

/// Limits on a workspace's patterns. A workspace beyond either limit includes nothing: its
/// patterns aren't read in part, so a dropped exclusion can never make a project eligible.
const MAX_PATTERNS: usize = 256;
const MAX_PATTERN_SEGMENTS: usize = 32;

/// A pattern's path segments, with adjacent `**` collapsed (`**/**` is the same as `**`).
fn pattern_parts(pattern: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = Vec::new();
    for part in pattern
        .trim_start_matches("./")
        .trim_end_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
    {
        if part == "**" && parts.last() == Some(&"**") {
            continue;
        }
        parts.push(part);
    }
    parts
}

pub(crate) fn workspace_member(patterns: &[String], relative: &str) -> bool {
    let segments: Vec<&str> = relative
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    let parsed: Vec<(bool, Vec<&str>)> = patterns
        .iter()
        .map(|pattern| match pattern.strip_prefix('!') {
            Some(excluded) => (true, pattern_parts(excluded)),
            None => (false, pattern_parts(pattern)),
        })
        .collect();
    // Fail closed rather than read part of a workspace's patterns.
    if parsed.len() > MAX_PATTERNS
        || parsed
            .iter()
            .any(|(_, parts)| parts.len() > MAX_PATTERN_SEGMENTS)
    {
        return false;
    }
    let included = parsed
        .iter()
        .any(|(excluded, parts)| !excluded && glob_segments(parts, &segments));
    let excluded = parsed
        .iter()
        .any(|(excluded, parts)| *excluded && glob_segments(parts, &segments));
    included && !excluded
}

/// Glob over path segments: `**` matches any number of segments, and `*` within a segment
/// matches any run of characters. Iterative, over a (pattern × path) table, so the work is
/// bounded by their lengths whatever the pattern.
fn glob_segments(pattern: &[&str], path: &[&str]) -> bool {
    // `reach[j]`: the pattern so far matches the first `j` path segments.
    let mut reach = vec![false; path.len() + 1];
    reach[0] = true;
    for part in pattern {
        let mut next = vec![false; path.len() + 1];
        if *part == "**" {
            // Any number of segments: everything reachable so far, and anything after it.
            let mut seen = false;
            for (j, slot) in next.iter_mut().enumerate() {
                seen |= reach[j];
                *slot = seen;
            }
        } else {
            for j in 0..path.len() {
                if reach[j] && glob_segment(part, path[j]) {
                    next[j + 1] = true;
                }
            }
        }
        reach = next;
    }
    reach[path.len()]
}

fn glob_segment(pattern: &str, segment: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == segment;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !segment.starts_with(first)
        || !segment[first.len()..].ends_with(last)
        || segment.len() < first.len() + last.len()
    {
        return false;
    }
    let mut rest = &segment[first.len()..segment.len() - last.len()];
    for part in &parts[1..parts.len() - 1] {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    true
}

pub struct Intake<'a> {
    pub home: &'a Path,
    pub running: &'a [RunningBundle],
    pub store_dev: Option<u64>,
    pub now_ms: u64,
    /// Folders excluded in Settings now: an item in or containing one is dropped before
    /// anything about it is read.
    pub excluded: &'a [PathBuf],
}

impl Intake<'_> {
    fn candidate(
        &self,
        kind: Kind,
        item: Source,
        path: std::path::PathBuf,
        stat: &crate::storage::fsdir::FileStat,
        facts: StoredFacts,
        name: String,
    ) -> Candidate {
        let same_volume = self.store_dev.is_none_or(|dev| dev == stat.dev);
        let file_name = super::display_name_of(&path);
        Candidate {
            id: new_id(self.now_ms),
            assessment: assess_now(&kind, &file_name, facts, self.running, same_volume),
            location: location_of(&path, Some(self.home)),
            identity: Identity::of(stat),
            name,
            bytes: 0,
            files: 0,
            lower_bound: false,
            modified_ms: None,
            kind,
            path,
            facts,
            source: item,
            project: None,
        }
    }

    pub fn developer_items(&self, items: Vec<ReviewItem>) -> Vec<Candidate> {
        items
            .into_iter()
            // Excluded since the Developer scan: dropped unread.
            .filter(|item| !crate::preferences::touches_excluded(&item.path, self.excluded))
            .map(|item| {
                let (kind, facts, name, project) = match item.kind {
                    ReviewKind::Cache(id) => (
                        Kind::Catalog(id),
                        StoredFacts::default(),
                        super::display_name_of(&item.path),
                        None,
                    ),
                    ReviewKind::Build {
                        name,
                        project,
                        last_commit_ms,
                    } => {
                        let facts = StoredFacts {
                            lockfile: name == "node_modules"
                                && has_lockfile(&project, self.home, self.excluded),
                            project_active: last_commit_ms.is_some_and(|commit| {
                                self.now_ms.saturating_sub(commit) < ACTIVE_DAYS * DAY_MS
                            }),
                            ..StoredFacts::default()
                        };
                        let shown = format!("{}/{name}", super::display_name_of(&project));
                        (Kind::BuildFolder { name }, facts, shown, Some(project))
                    }
                };
                let mut candidate =
                    self.candidate(kind, Source::Developer, item.path, &item.stat, facts, name);
                candidate.bytes = item.measured.bytes;
                candidate.files = item.measured.files;
                candidate.lower_bound = item.measured.lower_bound;
                candidate.modified_ms = u64::try_from(item.measured.newest_modified_s)
                    .ok()
                    .filter(|&seconds| seconds > 0)
                    .and_then(|seconds| seconds.checked_mul(1000));
                candidate.project = project;
                candidate
            })
            .collect()
    }

    /// A large file from the Storage page: always inspect only.
    pub fn storage_file(&self, file: LargeFile) -> Option<Candidate> {
        if crate::preferences::touches_excluded(&file.source.path, self.excluded) {
            return None;
        }
        let stat = file.source.stat?;
        let facts = StoredFacts {
            larger_than: STORAGE_LARGE_BYTES,
            ..StoredFacts::default()
        };
        let mut candidate = self.candidate(
            Kind::LargeFile,
            Source::Storage,
            file.source.path,
            &stat,
            facts,
            file.name,
        );
        candidate.bytes = file.bytes;
        candidate.files = 1;
        candidate.modified_ms = file.modified_ms;
        Some(candidate)
    }
}
