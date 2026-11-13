//! Sorts what the analysis counts into the Storage page's categories, by location. The rules
//! are a fixed, documented approximation (docs/storage-discovery.md): a folder's category
//! comes from where it is, never from its contents, and anything no rule names inherits the
//! category of the folder it is in.

use serde::Serialize;

use super::apps::AppIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Applications,
    Developer,
    User,
    System,
    Caches,
    /// App data and anything else no rule names.
    Other,
}

impl Category {
    pub const ALL: [Self; 6] = [
        Self::Applications,
        Self::Developer,
        Self::User,
        Self::System,
        Self::Caches,
        Self::Other,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Applications => 0,
            Self::Developer => 1,
            Self::User => 2,
            Self::System => 3,
            Self::Caches => 4,
            Self::Other => 5,
        }
    }
}

/// The fixed location a folder is under; each has its own rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootKind {
    Applications,
    Home,
    /// `/System` and `/usr` (without `/usr/local`).
    System,
    /// `/Library`, shared by every user of the Mac.
    Library,
    /// `/opt` and `/usr/local`: package managers such as Homebrew.
    Packages,
    /// `/private/var`.
    Var,
}

impl RootKind {
    /// The category of everything at the root that no rule names.
    pub const fn base(self) -> Category {
        match self {
            Self::Applications => Category::Applications,
            Self::Home => Category::User,
            Self::System | Self::Var => Category::System,
            Self::Library => Category::Other,
            Self::Packages => Category::Developer,
        }
    }
}

/// Where a rule's category applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The folder is one item in its category, such as `~/Library/Caches`.
    Itself,
    /// The folder is broad: each folder inside it is listed as its own item.
    Children,
}

/// Dot folders at the top of the home folder that hold developer tools and their caches.
const DEVELOPER_DOT_FOLDERS: [&str; 18] = [
    ".android",
    ".bun",
    ".cargo",
    ".cocoapods",
    ".conda",
    ".deno",
    ".docker",
    ".gradle",
    ".m2",
    ".npm",
    ".nvm",
    ".pnpm-store",
    ".pyenv",
    ".rustup",
    ".swiftpm",
    ".vscode",
    ".yarn",
    ".colima",
];

/// Folders that build tools and package managers create inside projects.
const PROJECT_FOLDERS: [&str; 8] = [
    "node_modules",
    ".venv",
    "__pycache__",
    "Pods",
    "DerivedData",
    ".gradle",
    ".next",
    ".turbo",
];

/// A folder directly inside `parent` (for example `Library/Application Support`) named after
/// an installed app is that app's data, counted with Applications as macOS does.
fn app_data(relative: &str, name: &str, parents: &[&str], apps: &AppIndex) -> bool {
    parents.iter().any(|parent| {
        relative
            .strip_prefix(parent)
            .and_then(|rest| rest.strip_prefix('/'))
            .is_some_and(|rest| rest == name)
    }) && apps.matches(name)
}

/// The rule for the folder at `relative` (its '/'-separated path below the root), if any.
/// `apps` are the installed apps, whose data folders count with Applications.
pub fn rule(
    root: RootKind,
    relative: &str,
    name: &str,
    apps: &AppIndex,
) -> Option<(Category, Scope)> {
    match root {
        RootKind::Home => home_rule(relative, name, apps),
        RootKind::Library => match relative {
            "Developer" => Some((Category::Developer, Scope::Children)),
            "Caches" | "Logs" => Some((Category::Caches, Scope::Itself)),
            _ if app_data(relative, name, &["Application Support", "Caches"], apps) => {
                Some((Category::Applications, Scope::Itself))
            }
            _ => None,
        },
        RootKind::Var => match relative {
            "folders" | "tmp" | "log" => Some((Category::Caches, Scope::Itself)),
            _ => None,
        },
        RootKind::Applications | RootKind::System | RootKind::Packages => None,
    }
}

fn home_rule(relative: &str, name: &str, apps: &AppIndex) -> Option<(Category, Scope)> {
    match relative {
        "Library" => return Some((Category::Other, Scope::Children)),
        "Library/Caches" | "Library/Logs" | ".cache" => {
            return Some((Category::Caches, Scope::Itself))
        }
        "Library/Developer" => return Some((Category::Developer, Scope::Children)),
        "Library/Containers/com.docker.docker" | "Library/Group Containers/group.com.docker" => {
            return Some((Category::Developer, Scope::Itself))
        }
        // Apps in the home folder, and the sandbox containers macOS keeps for each app.
        "Applications" | "Library/Containers" | "Library/Group Containers" => {
            return Some((Category::Applications, Scope::Children))
        }
        ".Trash" => return Some((Category::User, Scope::Itself)),
        _ => {}
    }
    if app_data(
        relative,
        name,
        &["Library/Application Support", "Library/Caches"],
        apps,
    ) {
        return Some((Category::Applications, Scope::Itself));
    }
    let top_level = !relative.contains('/');
    if top_level && name.starts_with('.') {
        let category = if DEVELOPER_DOT_FOLDERS.contains(&name) {
            Category::Developer
        } else {
            Category::Other
        };
        return Some((category, Scope::Itself));
    }
    if !relative.starts_with("Library/") && PROJECT_FOLDERS.contains(&name) {
        return Some((Category::Developer, Scope::Itself));
    }
    None
}

/// Every category that the folder at `relative` (of `category`) or anything inside it can
/// count toward. When the folder isn't counted, each of these is a minimum. Mirrors the rules
/// above: it must name every category a rule can assign below `relative`.
pub fn possible_below(root: RootKind, relative: &str, category: Category) -> Vec<Category> {
    let extra: &[Category] = match root {
        RootKind::Home => match relative {
            "" => &[
                Category::User,
                Category::Other,
                Category::Caches,
                Category::Developer,
                Category::Applications,
            ],
            "Library" => &[
                Category::Caches,
                Category::Developer,
                Category::Applications,
            ],
            "Library/Containers" | "Library/Group Containers" => &[Category::Developer],
            // An installed app's data folder inside them counts with Applications.
            "Library/Application Support" | "Library/Caches" => &[Category::Applications],
            _ if relative.starts_with("Library/") => &[],
            // Project folders such as node_modules can be anywhere outside Library.
            _ => &[Category::Developer],
        },
        RootKind::Library => match relative {
            "" => &[
                Category::Developer,
                Category::Caches,
                Category::Applications,
            ],
            "Application Support" | "Caches" => &[Category::Applications],
            _ => &[],
        },
        RootKind::Var if relative.is_empty() => &[Category::Caches],
        _ => &[],
    };
    let mut found = vec![category];
    for &candidate in extra {
        if !found.contains(&candidate) {
            found.push(candidate);
        }
    }
    found
}

/// Why a large file is shown without any suggestion: its owner manages it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Lock {
    /// Docker's disk image or data.
    Docker,
    /// Simulator runtimes and devices, removed from Xcode's settings.
    XcodeManaged,
    /// Part of macOS.
    Macos,
}

/// The lock for a large file at `relative` below a root of kind `root`.
pub fn lock(root: RootKind, relative: &str, category: Category) -> Option<Lock> {
    if root == RootKind::Home
        && (relative.starts_with("Library/Containers/com.docker.docker/")
            || relative.starts_with("Library/Group Containers/group.com.docker/"))
    {
        return Some(Lock::Docker);
    }
    let simulator = match root {
        RootKind::Home => relative.starts_with("Library/Developer/CoreSimulator/"),
        RootKind::Library => relative.starts_with("Developer/CoreSimulator/"),
        _ => false,
    };
    if simulator {
        return Some(Lock::XcodeManaged);
    }
    (category == Category::System).then_some(Lock::Macos)
}
