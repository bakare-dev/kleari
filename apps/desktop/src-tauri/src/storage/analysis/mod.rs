//! Read-only analysis of the startup disk for the Storage page: a fixed set of locations is
//! walked (never a path from the frontend), sizes are summed into a drill-down folder tree,
//! sorted into categories by location, and files of 1 GB or more are listed. Nothing is
//! stored; the result is returned to the window that asked for it.

mod apps;
mod classify;
mod walk;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use apps::AppIndex;
pub use classify::Category;
use classify::RootKind;
use walk::{walk_root, Accumulator, NodeKind, RootOutcome, RootSpec};
pub use walk::{Coverage, Item, LargeFile, Limits, Node};

use super::fsdir;

/// The Storage page's analysis steps, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StepId {
    Applications,
    Home,
    System,
    LibraryOptVar,
}

pub struct Step {
    pub id: StepId,
    pub name: String,
    /// The category a step that combines several locations is shown with.
    pub category: Category,
    pub roots: Vec<RootSpec>,
}

/// Folders inside the home folder that are not walked: cloud folders, whose providers can
/// download on listing. Their local space is left unmeasured instead.
const HOME_EXCLUDED: &[&str] = &["Library/CloudStorage", "Library/Mobile Documents"];
/// `/System/Volumes` mounts the other volumes (including the Data volume, which reports the
/// same device), so walking it would count the disk twice.
const SYSTEM_EXCLUDED: &[&str] = &["Volumes"];
/// `/usr/local` is walked with the package locations.
const USR_EXCLUDED: &[&str] = &["local"];
/// Swap and the sleep image are shown separately as space macOS manages.
const VAR_EXCLUDED: &[&str] = &["vm"];

fn root(path: &str, kind: RootKind, name: &str, excluded: &'static [&'static str]) -> RootSpec {
    RootSpec {
        path: PathBuf::from(path),
        kind,
        name: name.to_owned(),
        display_root: path.to_owned(),
        excluded,
    }
}

/// The fixed locations, in order. `home` is the user's home folder, when known.
pub fn steps(home: Option<&Path>) -> Vec<Step> {
    let home_root = home.map(|home| RootSpec {
        path: home.to_path_buf(),
        kind: RootKind::Home,
        name: home.file_name().map_or_else(
            || "Home".to_owned(),
            |name| format!("Users/{}", fsdir::display_name(name)),
        ),
        display_root: "~".to_owned(),
        excluded: HOME_EXCLUDED,
    });
    vec![
        Step {
            id: StepId::Applications,
            name: "Applications".to_owned(),
            category: Category::Applications,
            roots: vec![root(
                "/Applications",
                RootKind::Applications,
                "Applications",
                &[],
            )],
        },
        Step {
            id: StepId::Home,
            name: "Home".to_owned(),
            category: Category::User,
            roots: home_root.into_iter().collect(),
        },
        Step {
            id: StepId::System,
            name: "System".to_owned(),
            category: Category::System,
            roots: vec![
                root("/System", RootKind::System, "System", SYSTEM_EXCLUDED),
                root("/usr", RootKind::System, "usr", USR_EXCLUDED),
            ],
        },
        Step {
            id: StepId::LibraryOptVar,
            name: "Library, opt and var".to_owned(),
            category: Category::Other,
            roots: vec![
                root("/Library", RootKind::Library, "Library", &[]),
                root("/opt", RootKind::Packages, "opt", &[]),
                root("/private/var", RootKind::Var, "private/var", VAR_EXCLUDED),
                root("/usr/local", RootKind::Packages, "usr/local", &[]),
            ],
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AnalysisStatus {
    Completed,
    /// Stopped by the user; totals cover only what was read.
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepResult {
    pub id: StepId,
    pub bytes: u64,
    pub lower_bound: bool,
    /// `None` when none of the step's locations exist or the step didn't run.
    pub node: Option<Node>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryTotal {
    pub id: Category,
    pub bytes: u64,
    /// Something that would have counted toward it wasn't counted, so `bytes` is a minimum.
    pub lower_bound: bool,
    /// Largest items, largest first.
    pub top: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub status: AnalysisStatus,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub steps: Vec<StepResult>,
    pub categories: Vec<CategoryTotal>,
    /// Largest first.
    pub large_files: Vec<LargeFile>,
    pub large_files_truncated: bool,
    pub coverage: Coverage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisProgress {
    pub step: StepId,
    /// Bytes counted so far, across all steps.
    pub counted_bytes: u64,
    /// Bytes counted by each finished step, in order.
    pub finished_steps: Vec<u64>,
}

fn step_node(step: &Step, nodes: Vec<Node>) -> Option<Node> {
    if nodes.len() <= 1 {
        return nodes.into_iter().next();
    }
    let mut children = nodes;
    children.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
    Some(Node {
        name: step.name.clone(),
        kind: NodeKind::Folder,
        bytes: children.iter().map(|node| node.bytes).sum(),
        category: step.category,
        lower_bound: children.iter().any(|node| node.lower_bound),
        children: Some(children),
    })
}

/// Runs every step read-only. On cancellation the remaining steps are skipped.
pub fn analyze(
    steps: &[Step],
    limits: Limits,
    cancel: &AtomicBool,
    mut progress: impl FnMut(AnalysisProgress),
) -> (AnalysisStatus, Vec<StepResult>, Accumulator) {
    let mut acc = Accumulator::new(limits);
    // The apps in the Applications locations and in ~/Applications, read first so their data
    // folders in the home folder and /Library count with them.
    let app_folders: Vec<PathBuf> = steps
        .iter()
        .flat_map(|step| &step.roots)
        .filter_map(|root| match root.kind {
            RootKind::Applications => Some(root.path.clone()),
            RootKind::Home => Some(root.path.join("Applications")),
            _ => None,
        })
        .collect();
    let folders: Vec<&Path> = app_folders.iter().map(PathBuf::as_path).collect();
    // The mount table is read without file-system I/O, again before each folder and bundle;
    // if it can't be read, indexing stops rather than risk touching a network volume.
    acc.apps = AppIndex::read(&folders, &crate::system_monitor::non_local_mounts);
    let mut results = Vec::with_capacity(steps.len());
    let mut finished = Vec::with_capacity(steps.len());
    let mut cancelled = false;
    for step in steps {
        if cancelled {
            break;
        }
        progress(AnalysisProgress {
            step: step.id,
            counted_bytes: acc.counted_bytes,
            finished_steps: finished.clone(),
        });
        let mut nodes = Vec::new();
        for spec in &step.roots {
            let mut report = |counted_bytes| {
                progress(AnalysisProgress {
                    step: step.id,
                    counted_bytes,
                    finished_steps: finished.clone(),
                });
            };
            match walk_root(spec, &mut acc, cancel, &mut report) {
                RootOutcome::Missing => {}
                RootOutcome::Walked {
                    node,
                    cancelled: stopped,
                } => {
                    nodes.push(node);
                    if stopped {
                        cancelled = true;
                        break;
                    }
                }
            }
        }
        let node = step_node(step, nodes);
        let bytes = node.as_ref().map_or(0, |node| node.bytes);
        finished.push(bytes);
        results.push(StepResult {
            id: step.id,
            bytes,
            lower_bound: node.as_ref().is_some_and(|node| node.lower_bound),
            node,
        });
    }
    let status = if cancelled {
        AnalysisStatus::Cancelled
    } else {
        AnalysisStatus::Completed
    };
    (status, results, acc)
}

/// Assembles the result sent to the frontend.
pub fn summarize(
    status: AnalysisStatus,
    steps: Vec<StepResult>,
    acc: Accumulator,
    started_at_ms: u64,
    duration_ms: u64,
) -> Analysis {
    let mut items = acc.category_items;
    let categories = Category::ALL
        .iter()
        .map(|&id| CategoryTotal {
            id,
            bytes: acc.category_bytes[id.index()],
            lower_bound: acc.category_lower_bound[id.index()],
            top: std::mem::take(&mut items[id.index()]),
        })
        .collect();
    Analysis {
        status,
        started_at_ms,
        duration_ms,
        steps,
        categories,
        large_files: acc.large_files,
        large_files_truncated: acc.large_files_truncated,
        coverage: acc.coverage,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisBusy;

/// The running analysis, if any, and the last one's large files (so one can be added to
/// Cleanup's review). Nothing is persisted.
#[derive(Default)]
pub struct AnalysisState {
    active: Mutex<Option<Arc<AtomicBool>>>,
    large_files: Mutex<Vec<LargeFile>>,
}

impl AnalysisState {
    pub fn begin(&self) -> Result<Arc<AtomicBool>, AnalysisBusy> {
        let mut active = super::lock(&self.active);
        if active.is_some() {
            return Err(AnalysisBusy);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(Arc::clone(&cancel));
        Ok(cancel)
    }

    pub fn end(&self) {
        *super::lock(&self.active) = None;
    }

    pub fn set_large_files(&self, files: Vec<LargeFile>) {
        *super::lock(&self.large_files) = files;
    }

    /// The last analysis's large file with this review ID.
    pub fn large_file(&self, review_id: &str) -> Option<LargeFile> {
        super::lock(&self.large_files)
            .iter()
            .find(|file| file.review_id == review_id)
            .cloned()
    }

    /// Requests cancellation; returns whether an analysis was running.
    pub fn cancel(&self) -> bool {
        match &*super::lock(&self.active) {
            Some(cancel) => {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                true
            }
            None => false,
        }
    }
}
