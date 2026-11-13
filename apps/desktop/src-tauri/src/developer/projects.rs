//! Finds projects and their build folders for the Developer page, read-only. A project is a
//! folder holding a marker file (`package.json`, `Cargo.toml`, `pom.xml`, …); its build
//! folders are the outputs that marker's tools make again (`node_modules`, `target`, …).
//! Source code is never listed.
//!
//! The walk uses `storage::fsdir` handles: each folder is opened relative to its open parent
//! without following symlinks, with an identity check, and only on the root's volume. It is
//! depth-first with one open folder per level, so at most `max_depth + 1` folders are open
//! at once. The mount table is read again before each root, and a root under a non-local
//! mount (or any root, if the table can't be read) is skipped and reported. It reads only
//! folder listings and opens one file per Git repository, `.git/logs/HEAD`, for the time of
//! the last commit. It never descends into build folders, `.git` or hidden folders.

use std::collections::HashSet;
use std::ffi::{CString, OsStr};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use super::measure::{open_absolute, open_beneath};
use crate::storage::fsdir::{display_name, DirHandle};

/// Listing entries read per folder; a folder with more is listed partially.
const MAX_ENTRIES: usize = 20_000;
/// Bytes read from the end of `.git/logs/HEAD` to find the last entry.
const HEAD_TAIL: u64 = 8 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Folder levels below each root searched for projects.
    pub max_depth: usize,
    pub max_projects: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 8,
            max_projects: 500,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stack {
    Node,
    NextJs,
    Rust,
    Maven,
    Gradle,
    Python,
    CocoaPods,
    Flutter,
    Php,
}

/// The build folders a marker's tools make. Order matters only for display.
fn outputs(stack: Stack) -> &'static [&'static str] {
    match stack {
        Stack::Node | Stack::NextJs => &[
            "node_modules",
            ".next",
            ".turbo",
            ".nuxt",
            ".svelte-kit",
            ".parcel-cache",
            "dist",
            "build",
            "out",
        ],
        Stack::Rust | Stack::Maven => &["target"],
        Stack::Gradle => &["build", ".gradle"],
        Stack::Python => &[".venv", "venv", ".pytest_cache", ".mypy_cache", ".tox"],
        Stack::CocoaPods => &["Pods"],
        Stack::Flutter => &["build", ".dart_tool"],
        Stack::Php => &["vendor"],
    }
}

/// The stacks a folder's file names show, most specific first.
pub fn stacks_of(names: &HashSet<String>) -> Vec<Stack> {
    let has = |name: &str| names.contains(name);
    let mut stacks = Vec::new();
    if has("package.json") {
        let next = names.iter().any(|name| name.starts_with("next.config."));
        stacks.push(if next { Stack::NextJs } else { Stack::Node });
    }
    if has("Cargo.toml") {
        stacks.push(Stack::Rust);
    }
    if has("pom.xml") {
        stacks.push(Stack::Maven);
    }
    if [
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
    ]
    .iter()
    .any(|name| has(name))
    {
        stacks.push(Stack::Gradle);
    }
    if ["pyproject.toml", "requirements.txt", "setup.py", "Pipfile"]
        .iter()
        .any(|name| has(name))
    {
        stacks.push(Stack::Python);
    }
    if has("Podfile") {
        stacks.push(Stack::CocoaPods);
    }
    if has("pubspec.yaml") {
        stacks.push(Stack::Flutter);
    }
    if has("composer.json") {
        stacks.push(Stack::Php);
    }
    stacks
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundProject {
    pub path: PathBuf,
    pub name: String,
    pub stack: Stack,
    /// Build folders present, by name.
    pub outputs: Vec<String>,
    /// The repository's last commit, from `.git/logs/HEAD`, in ms since the Unix epoch.
    pub last_commit_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Discovery {
    /// Folders that couldn't be listed; projects inside them are missing.
    pub unreadable: u64,
    pub truncated: bool,
    /// Roots not searched: under a non-local mount, the mount table couldn't be read, or
    /// the root couldn't be opened.
    pub skipped_roots: Vec<PathBuf>,
}

/// The time of the last entry in a `logs/HEAD` file: `<old> <new> <name> <email> <seconds> <tz>\t<message>`.
pub fn last_commit_seconds(tail: &[u8]) -> Option<u64> {
    let text = String::from_utf8_lossy(tail);
    let line = text.lines().rev().find(|line| !line.trim().is_empty())?;
    let header = line.split('\t').next()?;
    let mut fields = header.rsplit(' ');
    let _zone = fields.next()?;
    fields.next()?.parse::<u64>().ok()
}

/// Reads `.git/logs/HEAD` below an open folder, staying on its volume `dev`.
fn read_last_commit(repository: &DirHandle, dev: u64) -> Option<u64> {
    let logs = open_beneath(repository, dev, Path::new(".git/logs"))?;
    let mut file = logs.open_file_child(c"HEAD").ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.dev() != dev {
        return None;
    }
    let start = metadata.len().saturating_sub(HEAD_TAIL);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = Vec::new();
    file.take(HEAD_TAIL).read_to_end(&mut tail).ok()?;
    last_commit_seconds(&tail)?.checked_mul(1000)
}

/// One open folder on the current path and the subfolders still to visit.
struct Frame {
    dir: DirHandle,
    path: PathBuf,
    depth: usize,
    /// The last commit of the nearest enclosing repository.
    commit_ms: Option<u64>,
    children: std::vec::IntoIter<CString>,
}

fn under(path: &Path, mounts: &[PathBuf]) -> bool {
    mounts.iter().any(|mount| path.starts_with(mount))
}

struct Walk<'a, F> {
    limits: Limits,
    skip: &'a [PathBuf],
    cancel: &'a AtomicBool,
    discovery: Discovery,
    seen: HashSet<PathBuf>,
    found: usize,
    on_project: F,
}

impl<F: FnMut(FoundProject, &DirHandle, u64)> Walk<'_, F> {
    /// Cancelled, or the project cap reached (which marks the search truncated).
    fn stopped(&mut self) -> bool {
        if self.cancel.load(Ordering::Relaxed) {
            return true;
        }
        if self.found >= self.limits.max_projects {
            self.discovery.truncated = true;
            return true;
        }
        false
    }

    /// Lists an open folder, reports it if it is a project, and returns it with the
    /// subfolders to visit. `None` if the scan was cancelled while listing.
    fn enter(
        &mut self,
        mut dir: DirHandle,
        path: PathBuf,
        depth: usize,
        commit_ms: Option<u64>,
        dev: u64,
    ) -> Option<Frame> {
        let mut names: Vec<CString> = Vec::new();
        let mut text: HashSet<String> = HashSet::new();
        while let Some(next) = dir.next_name() {
            if self.cancel.load(Ordering::Relaxed) {
                return None;
            }
            match next {
                Ok(name) if names.len() < MAX_ENTRIES => {
                    text.insert(display_name(OsStr::from_bytes(name.to_bytes())));
                    names.push(name);
                }
                Ok(_) => {
                    self.discovery.truncated = true;
                    break;
                }
                Err(_) => {
                    self.discovery.unreadable += 1;
                    break;
                }
            }
        }
        let stacks = stacks_of(&text);
        // This project's build folders, which the search doesn't descend into.
        let mut build: Vec<String> = Vec::new();
        if let Some(&stack) = stacks.first() {
            for &candidate in stacks.iter().flat_map(|&stack| outputs(stack)) {
                if build.iter().any(|name| name == candidate) || !text.contains(candidate) {
                    continue;
                }
                // An excluded build folder, or one holding an excluded folder, isn't looked at.
                if self.touches_skip(&path.join(candidate)) {
                    continue;
                }
                let is_dir = CString::new(candidate)
                    .ok()
                    .and_then(|name| dir.stat_child(&name).ok())
                    .is_some_and(|stat| stat.is_dir() && stat.dev == dev);
                if is_dir {
                    build.push(candidate.to_owned());
                }
            }
            if !build.is_empty() && self.seen.insert(path.clone()) {
                self.found += 1;
                let project = FoundProject {
                    name: path.file_name().map_or_else(String::new, display_name),
                    path: path.clone(),
                    stack,
                    outputs: build.clone(),
                    last_commit_ms: commit_ms,
                };
                (self.on_project)(project, &dir, dev);
            }
        }
        let children: Vec<CString> = if depth >= self.limits.max_depth {
            Vec::new()
        } else {
            names
                .into_iter()
                .filter(|name| {
                    let shown = display_name(OsStr::from_bytes(name.to_bytes()));
                    !(shown.starts_with('.') || shown == "node_modules" || build.contains(&shown))
                })
                .collect()
        };
        Some(Frame {
            dir,
            path,
            depth,
            commit_ms,
            children: children.into_iter(),
        })
    }

    /// Whether `path` is in a skipped folder or contains one: it isn't read at all.
    fn touches_skip(&self, path: &Path) -> bool {
        self.skip
            .iter()
            .any(|skipped| path.starts_with(skipped) || skipped.starts_with(path))
    }

    /// The last commit's time from `<project>/.git/logs/HEAD`, unless that is excluded.
    fn last_commit(&self, project: &Path, dir: &DirHandle, dev: u64) -> Option<u64> {
        if self.touches_skip(&project.join(".git/logs/HEAD")) {
            return None;
        }
        read_last_commit(dir, dev)
    }

    fn search(&mut self, root: &Path, dir: DirHandle, dev: u64) {
        let commit_ms = self.last_commit(root, &dir, dev);
        let Some(frame) = self.enter(dir, root.to_path_buf(), 0, commit_ms, dev) else {
            return;
        };
        let mut stack = vec![frame];
        while !self.stopped() {
            let Some(frame) = stack.last_mut() else {
                return;
            };
            let Some(name) = frame.children.next() else {
                stack.pop();
                continue;
            };
            let path = frame
                .path
                .join(display_name(OsStr::from_bytes(name.to_bytes())));
            if self.skip.iter().any(|skipped| skipped == &path) {
                continue;
            }
            let Ok(stat) = frame.dir.stat_child(&name) else {
                continue;
            };
            if !stat.is_dir() || stat.dev != dev {
                continue;
            }
            let Ok((child, opened)) = frame.dir.open_child(&name) else {
                self.discovery.unreadable += 1;
                continue;
            };
            if !opened.is_dir() || opened.dev != stat.dev || opened.ino != stat.ino {
                continue;
            }
            let depth = frame.depth + 1;
            let commit_ms = self.last_commit(&path, &child, dev).or(frame.commit_ms);
            let Some(next) = self.enter(child, path, depth, commit_ms, dev) else {
                return;
            };
            stack.push(next);
        }
    }
}

/// Searches `roots` for projects. `skip` lists folders not searched (the home folder's
/// Library and Trash); `mounts` reads the non-local mounts (`None` if the table can't be
/// read). Each project is handed to `on_project` with its open folder and volume while the
/// search is still holding it, so its build folders can be opened relative to that handle.
pub fn discover(
    roots: &[PathBuf],
    skip: &[PathBuf],
    limits: Limits,
    cancel: &AtomicBool,
    mounts: impl Fn() -> Option<Vec<PathBuf>>,
    on_project: impl FnMut(FoundProject, &DirHandle, u64),
) -> Discovery {
    let mut walk = Walk {
        limits,
        skip,
        cancel,
        discovery: Discovery::default(),
        seen: HashSet::new(),
        found: 0,
        on_project,
    };
    for root in roots {
        if walk.stopped() {
            break;
        }
        // `open_absolute` refuses symlinks along the whole path, so the lexical path checked
        // against the mount table is the one actually opened.
        let local = mounts().is_some_and(|mounts| !under(root, &mounts));
        let opened = if local { open_absolute(root) } else { None };
        match opened {
            Some((dir, dev)) => walk.search(root, dir, dev),
            None => walk.discovery.skipped_roots.push(root.clone()),
        }
    }
    walk.discovery
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(tag: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("kleari-projects-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("temp");
        fs::canonicalize(&path).expect("canonical")
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().expect("parent")).expect("folders");
        fs::write(path, b"x").expect("file");
    }

    fn local() -> Option<Vec<PathBuf>> {
        Some(Vec::new())
    }

    fn find(
        roots: &[PathBuf],
        skip: &[PathBuf],
        limits: Limits,
        cancel: &AtomicBool,
    ) -> (Vec<FoundProject>, Discovery) {
        let mut found = Vec::new();
        let discovery = discover(roots, skip, limits, cancel, local, |project, _, _| {
            found.push(project);
        });
        (found, discovery)
    }

    #[test]
    fn finds_projects_and_build_folders_without_entering_them_or_hidden_folders() {
        let root = temp("discover");
        touch(&root.join("web/package.json"));
        touch(&root.join("web/next.config.js"));
        touch(&root.join("web/node_modules/dep/package.json"));
        touch(&root.join("web/.next/cache"));
        touch(&root.join("web/src/index.ts"));
        touch(&root.join("api/Cargo.toml"));
        touch(&root.join("api/target/debug/app"));
        fs::create_dir_all(root.join("api/.git/logs")).expect("git");
        fs::write(
            root.join("api/.git/logs/HEAD"),
            "0 1 A <a@b> 1750000000 +0000\tcommit\n",
        )
        .expect("log");
        touch(&root.join("monorepo/apps/site/package.json"));
        touch(&root.join("monorepo/apps/site/dist/index.js"));
        touch(&root.join("notes/README.md"));
        touch(&root.join(".hidden/package.json"));
        touch(&root.join(".hidden/node_modules/x"));
        touch(&root.join("Library/thing/package.json"));
        touch(&root.join("Library/thing/node_modules/x"));
        // A marker without build output isn't listed.
        touch(&root.join("fresh/package.json"));

        let cancel = AtomicBool::new(false);
        let (mut projects, discovery) = find(
            std::slice::from_ref(&root),
            &[root.join("Library")],
            Limits::default(),
            &cancel,
        );
        projects.sort_by(|a, b| a.name.cmp(&b.name));
        let summary: Vec<(&str, Stack, Vec<&str>)> = projects
            .iter()
            .map(|project| {
                (
                    project.name.as_str(),
                    project.stack,
                    project.outputs.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("api", Stack::Rust, vec!["target"]),
                ("site", Stack::Node, vec!["dist"]),
                ("web", Stack::NextJs, vec!["node_modules", ".next"]),
            ]
        );
        let api = projects
            .iter()
            .find(|project| project.name == "api")
            .expect("api");
        assert_eq!(api.last_commit_ms, Some(1_750_000_000_000));
        assert_eq!(discovery, Discovery::default());
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_symlinked_git_folder_is_not_read() {
        let root = temp("gitlink");
        touch(&root.join("real/.git/logs/HEAD"));
        fs::write(
            root.join("real/.git/logs/HEAD"),
            "0 1 A <a@b> 1750000000 +0000\tcommit\n",
        )
        .expect("log");
        touch(&root.join("api/Cargo.toml"));
        touch(&root.join("api/target/app"));
        std::os::unix::fs::symlink(root.join("real/.git"), root.join("api/.git")).expect("link");
        let cancel = AtomicBool::new(false);
        let (projects, _) = find(&[root.join("api")], &[], Limits::default(), &cancel);
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].last_commit_ms, None);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn an_excluded_git_folder_or_build_folder_is_not_read() {
        let root = temp("excluded-git");
        touch(&root.join("api/Cargo.toml"));
        touch(&root.join("api/target/app"));
        touch(&root.join("api/.git/logs/HEAD"));
        fs::write(
            root.join("api/.git/logs/HEAD"),
            "0 1 A <a@b> 1750000000 +0000\tcommit\n",
        )
        .expect("log");
        let cancel = AtomicBool::new(false);
        let (read, _) = find(&[root.join("api")], &[], Limits::default(), &cancel);
        assert_eq!(read[0].last_commit_ms, Some(1_750_000_000_000));
        // Excluding .git, or the log folder inside it, stops the read.
        for excluded in [root.join("api/.git"), root.join("api/.git/logs")] {
            let (projects, _) = find(&[root.join("api")], &[excluded], Limits::default(), &cancel);
            assert_eq!(projects[0].last_commit_ms, None);
        }
        // An excluded build folder isn't treated as one, so the project has none to offer.
        let (projects, _) = find(
            &[root.join("api")],
            &[root.join("api/target")],
            Limits::default(),
            &cancel,
        );
        assert!(projects.is_empty());
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn roots_that_cant_be_searched_are_reported() {
        let root = temp("skipped");
        touch(&root.join("p/package.json"));
        touch(&root.join("p/node_modules/x"));
        let missing = root.join("missing");
        let cancel = AtomicBool::new(false);
        let (projects, discovery) = find(
            &[missing.clone(), root.clone()],
            &[],
            Limits::default(),
            &cancel,
        );
        assert_eq!(projects.len(), 1);
        assert_eq!(discovery.skipped_roots, [missing]);
        // Under a non-local mount, or with no mount table, nothing is searched.
        let mount = root.clone();
        let remote = discover(
            std::slice::from_ref(&root),
            &[],
            Limits::default(),
            &cancel,
            || Some(vec![mount.clone()]),
            |_, _, _| panic!("searched a non-local root"),
        );
        assert_eq!(remote.skipped_roots, std::slice::from_ref(&root));
        let unknown = discover(
            std::slice::from_ref(&root),
            &[],
            Limits::default(),
            &cancel,
            || None,
            |_, _, _| panic!("searched without a mount table"),
        );
        assert_eq!(unknown.skipped_roots, std::slice::from_ref(&root));
        fs::remove_dir_all(&root).expect("cleanup");
    }
}
