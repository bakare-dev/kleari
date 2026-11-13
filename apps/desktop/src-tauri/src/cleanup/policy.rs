//! What Kleari may offer to quarantine, decided in Rust only. The frontend displays these
//! decisions; it can't make an item eligible. Risk (what you lose) and eligibility (whether
//! Kleari will move it now) are separate: a low risk never makes an item eligible by itself.
//!
//! Two checks apply to every item:
//! - `assess` classifies it from what the scan observed (running apps, a lockfile, the last
//!   commit, its age, its volume).
//! - `within_bounds` confirms its path is one of the fixed places its kind may come from.
//!   It runs again immediately before anything moves.

use std::path::{Component, Path};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    /// App caches in `~/Library/Caches`.
    Cache,
    /// Xcode and simulator data.
    Xcode,
    /// Package-manager caches.
    Pkg,
    /// `node_modules` folders.
    Node,
    /// Other build output in projects.
    Build,
    /// Logs, crash reports and temporary files.
    Logs,
    /// Large files (inspect only).
    Large,
    /// Files not opened for a long time (inspect only).
    Old,
    /// What apps leave behind: data of removed apps, and installers already used.
    Apps,
    /// Files and folders you chose yourself on the Storage page.
    Chosen,
}

impl Category {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cache => "cache",
            Self::Xcode => "xcode",
            Self::Pkg => "pkg",
            Self::Node => "node",
            Self::Build => "build",
            Self::Logs => "logs",
            Self::Large => "large",
            Self::Old => "old",
            Self::Apps => "apps",
            Self::Chosen => "chosen",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Risk {
    Low,
    Medium,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Certainty {
    High,
    Medium,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Eligibility {
    /// May be selected for a preview.
    Eligible,
    /// Not while something is using it (an app or Xcode is open, or the project is active).
    NotNow,
    /// Not until something is checked (a lockfile).
    NeedsCheck,
    /// It changed since the scan; scan again.
    Rescan,
    /// Listed for you to look at; Kleari never moves it.
    InspectOnly,
}

/// Why an item isn't eligible. The frontend has the wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WhyNot {
    AppRunning,
    XcodeRunning,
    SimulatorRunning,
    NoLockfile,
    ActiveProject,
    /// macOS and Apple's apps use these while running.
    AppleCache,
    /// Delete simulators and runtimes in Xcode so it stays consistent.
    ManageInXcode,
    /// Large and old files are yours to decide about in Finder.
    UserFile,
    /// It's on another disk than the quarantine folder.
    OtherVolume,
    ChangedSinceScan,
    /// Something inside is open in an app.
    OpenInApp,
}

/// What Kleari observed about an item. The frontend has the wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceCode {
    InCaches,
    AppNotRunning,
    AppRunning,
    NoAppMatch,
    AppleCache,
    PackageCache,
    RefetchedOnInstall,
    InDerivedData,
    XcodeNotRunning,
    XcodeRunning,
    RebuildSlower,
    Archive,
    LosesDebugSymbols,
    DeviceSupport,
    CopiedAgainOnConnect,
    SimulatorCache,
    SimulatorRunning,
    SimulatorData,
    SharedStore,
    Toolchain,
    LogOlderThan,
    TempOlderThan,
    NotWrittenSince,
    LockfileFound,
    NoLockfile,
    ProjectActive,
    ProjectInactive,
    BuildOutput,
    ReinstallNeeded,
    LargerThan,
    NotOpenedFor,
    NotKnownCache,
    OtherVolume,
    LeftoverOfRemovedApp,
    LeftoverNoApp,
    LosesAppData,
    InstallerFile,
    DownloadAgain,
    ChosenByYou,
    OpenInApp,
    NothingOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    Ok,
    Warn,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub tone: Tone,
    pub code: EvidenceCode,
    /// A number the wording needs (days, or bytes for a size threshold).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<u64>,
    /// An app's name the wording needs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
}

fn ok(code: EvidenceCode) -> Evidence {
    Evidence {
        tone: Tone::Ok,
        code,
        value: None,
        app: None,
    }
}

fn warn(code: EvidenceCode) -> Evidence {
    Evidence {
        tone: Tone::Warn,
        code,
        value: None,
        app: None,
    }
}

fn info(code: EvidenceCode) -> Evidence {
    Evidence {
        tone: Tone::Info,
        code,
        value: None,
        app: None,
    }
}

impl Evidence {
    fn with_value(mut self, value: u64) -> Self {
        self.value = Some(value);
        self
    }

    fn with_app(mut self, app: &str) -> Self {
        self.app = Some(app.to_owned());
        self
    }
}

/// Where an item came from and what it is. Decides its category and the places it may be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A folder directly in `~/Library/Caches`.
    AppCache,
    /// A folder in `~/Library/Caches` that belongs to a package manager.
    CachesPackage,
    /// A Developer-catalog location, by catalog ID.
    Catalog(&'static str),
    /// A folder or file directly in `~/Library/Logs`.
    Log,
    /// A file directly in `~/Library/Logs/DiagnosticReports`.
    CrashReport,
    /// A folder or file directly in the user's temporary folder.
    Temp,
    /// A folder directly in Xcode's DerivedData.
    DerivedData,
    /// A folder directly in Xcode's Archives.
    Archive,
    /// A folder directly in an `* DeviceSupport` folder.
    DeviceSupport,
    /// `~/Library/Developer/CoreSimulator/Caches`.
    SimulatorCache,
    /// A folder directly in `~/Library/Developer/CoreSimulator/Devices`.
    SimulatorDevice,
    /// A build folder named `name` directly in a project.
    BuildFolder { name: String },
    /// A large file found by a scan or sent from Storage.
    LargeFile,
    /// A file not opened for a long time.
    OldFile,
    /// Data an app keeps in the user's Library (see `LEFTOVER_FOLDERS`), named for the app
    /// with bundle ID or name `owner`, found after the app was removed.
    AppLeftover { owner: String },
    /// An installer (`.dmg`, `.pkg`, `.mpkg`, `.xip`) directly in Downloads or on the Desktop.
    Installer,
    /// A file or folder you chose on the Storage page (see `chosen_in_bounds`).
    UserChosen,
}

/// Package managers whose caches live in `~/Library/Caches` under these names.
pub const CACHES_PACKAGES: &[&str] = &[
    "Homebrew",
    "pnpm",
    "Yarn",
    "CocoaPods",
    "pip",
    "go-build",
    "node-gyp",
    "deno",
    "pypoetry",
    "composer",
];

pub fn category(kind: &Kind) -> Category {
    match kind {
        Kind::AppCache => Category::Cache,
        Kind::CachesPackage => Category::Pkg,
        Kind::Catalog(id) if catalog_is_xcode(id) => Category::Xcode,
        Kind::Catalog(_) => Category::Pkg,
        Kind::Log | Kind::CrashReport | Kind::Temp => Category::Logs,
        Kind::DerivedData
        | Kind::Archive
        | Kind::DeviceSupport
        | Kind::SimulatorCache
        | Kind::SimulatorDevice => Category::Xcode,
        Kind::BuildFolder { name } if name == "node_modules" => Category::Node,
        Kind::BuildFolder { .. } => Category::Build,
        Kind::LargeFile => Category::Large,
        Kind::OldFile => Category::Old,
        Kind::AppLeftover { .. } | Kind::Installer => Category::Apps,
        Kind::UserChosen => Category::Chosen,
    }
}

/// Folders directly in the home folder that are never chosen as a whole (their contents can
/// be): moving one would break macOS's layout or sync.
const HOME_FOLDERS: &[&str] = &[
    "Desktop",
    "Documents",
    "Downloads",
    "Library",
    "Movies",
    "Music",
    "Pictures",
    "Public",
    "Applications",
    ".Trash",
];

/// The only parts of the Library where you may choose something yourself.
const CHOSEN_IN_LIBRARY: &[&str] = &["Library/Caches", "Library/Logs", "Library/Developer"];

/// Packages whose insides belong to their app: the package may be chosen, never part of it.
const PACKAGE_SUFFIXES: &[&str] = &[
    ".app",
    ".photoslibrary",
    ".musiclibrary",
    ".fcpbundle",
    ".bundle",
    ".framework",
    ".xcarchive",
    ".imovielibrary",
    ".tvlibrary",
    ".logicx",
];

/// Whether `relative` (below the home folder, already free of the never-touched places) may be
/// chosen: not a top-level home folder itself, in the Library only below Caches, Logs or
/// Developer, never in the Trash, and never inside a package.
fn chosen_in_bounds(relative: &Path) -> bool {
    let parts: Vec<&str> = relative
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .collect();
    if parts.len() != relative.components().count() || parts.is_empty() {
        return false;
    }
    if parts.len() == 1 && HOME_FOLDERS.contains(&parts[0]) {
        return false;
    }
    if parts[0] == ".Trash" {
        return false;
    }
    if parts[0] == "Library"
        && !CHOSEN_IN_LIBRARY
            .iter()
            .any(|allowed| relative.starts_with(allowed) && relative != Path::new(allowed))
    {
        return false;
    }
    !parts[..parts.len() - 1].iter().any(|part| {
        let lower = part.to_lowercase();
        PACKAGE_SUFFIXES
            .iter()
            .any(|suffix| lower.ends_with(suffix))
    })
}

/// Folders in the home folder where apps keep their data, each entry named for its app.
pub const LEFTOVER_FOLDERS: &[&str] = &[
    "Library/Application Support",
    "Library/Caches",
    "Library/Preferences",
    "Library/Preferences/ByHost",
    "Library/Saved Application State",
    "Library/Containers",
    "Library/Group Containers",
    "Library/HTTPStorages",
    "Library/WebKit",
    "Library/Logs",
    "Library/Cookies",
    "Library/Application Scripts",
    "Library/LaunchAgents",
];

/// Installer file extensions, lowercase.
pub const INSTALLER_EXTENSIONS: &[&str] = &["dmg", "pkg", "mpkg", "xip"];

/// Whether `name`, an entry directly in a leftover folder, belongs to the app with bundle ID
/// or name `owner`: the same name (ignoring case and a `.plist`, `.savedState` or
/// `.binarycookies` suffix), the name followed by a dot (`ByHost/<id>.<host>.plist`, a launch
/// agent `<id>.helper.plist`), or a group container `<team>.<id>`. Apple's own never match.
pub fn leftover_matches(name: &str, owner: &str) -> bool {
    let name = name.to_lowercase();
    let owner = owner.trim().to_lowercase();
    if owner.chars().count() < 3
        || owner.starts_with("com.apple.")
        || name.starts_with("com.apple.")
        || owner.contains('/')
    {
        return false;
    }
    let stem = [".plist", ".savedstate", ".binarycookies"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(&name);
    stem == owner
        || stem
            .strip_prefix(owner.as_str())
            .is_some_and(|rest| rest.starts_with('.'))
        || (owner.contains('.') && stem.ends_with(&format!(".{owner}")))
}

/// Whether `path` is an entry directly in one of `LEFTOVER_FOLDERS` of `home`, not Apple's.
pub fn in_leftover_folder(path: &Path, home: &Path) -> bool {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
    else {
        return false;
    };
    plain(path)
        && plain(home)
        && !name.to_lowercase().starts_with("com.apple.")
        && LEFTOVER_FOLDERS
            .iter()
            .any(|folder| parent == home.join(folder))
}

/// Whether `name` is an installer file by its extension.
pub fn is_installer_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| INSTALLER_EXTENSIONS.contains(&extension.to_lowercase().as_str()))
}

fn catalog_is_xcode(id: &str) -> bool {
    id.starts_with("xcode") || id.starts_with("simulator")
}

/// Facts the scan observed, used to classify an item.
#[derive(Debug, Clone, Default)]
pub struct Facts<'a> {
    /// The item's name.
    pub name: &'a str,
    /// The running app that owns it (its bundle ID matches the name), if any.
    pub running_app: Option<&'a str>,
    pub xcode_running: bool,
    pub simulator_running: bool,
    /// For `node_modules`: whether a lockfile sits next to it.
    pub lockfile: bool,
    /// For build folders: the project's last commit was within the active window.
    pub project_active: bool,
    /// Days since anything inside changed (logs, temp) or since it was opened (old files).
    pub idle_days: u64,
    /// The size threshold a large file passed.
    pub larger_than: u64,
    /// It is on the quarantine folder's volume.
    pub same_volume: bool,
    /// For app leftovers: the app was removed with Kleari just now, so they are certainly
    /// its data rather than matched by name alone.
    pub owner_removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    pub risk: Risk,
    pub certainty: Certainty,
    pub eligibility: Eligibility,
    pub why_not: Option<WhyNot>,
    pub evidence: Vec<Evidence>,
}

fn eligible(risk: Risk, certainty: Certainty, evidence: Vec<Evidence>) -> Assessment {
    Assessment {
        risk,
        certainty,
        eligibility: Eligibility::Eligible,
        why_not: None,
        evidence,
    }
}

fn blocked(
    risk: Risk,
    certainty: Certainty,
    eligibility: Eligibility,
    why_not: WhyNot,
    evidence: Vec<Evidence>,
) -> Assessment {
    Assessment {
        risk,
        certainty,
        eligibility,
        why_not: Some(why_not),
        evidence,
    }
}

/// Whether `name` looks like a bundle identifier (`com.example.App`).
fn bundle_like(name: &str) -> bool {
    name.split('.').filter(|part| !part.is_empty()).count() >= 2
}

/// Catalog caches whose removal costs more than a download: shared stores and toolchains.
fn catalog_risk(id: &str) -> Risk {
    match id {
        "xcodeArchives" | "pnpmStore" | "rustup" => Risk::Medium,
        _ => Risk::Low,
    }
}

/// Classifies one item. Eligibility is decided here and nowhere else.
pub fn assess(kind: &Kind, facts: &Facts<'_>) -> Assessment {
    let mut assessment = assess_kind(kind, facts);
    // Only same-volume moves are allowed: an item on another disk is listed, never moved.
    if assessment.eligibility == Eligibility::Eligible && !facts.same_volume {
        assessment.eligibility = Eligibility::InspectOnly;
        assessment.why_not = Some(WhyNot::OtherVolume);
        assessment.evidence.push(warn(EvidenceCode::OtherVolume));
    }
    assessment
}

fn assess_kind(kind: &Kind, facts: &Facts<'_>) -> Assessment {
    use EvidenceCode as E;
    match kind {
        Kind::AppCache => {
            if facts.name.starts_with("com.apple.") {
                return blocked(
                    Risk::Medium,
                    Certainty::High,
                    Eligibility::InspectOnly,
                    WhyNot::AppleCache,
                    vec![ok(E::InCaches), warn(E::AppleCache)],
                );
            }
            if let Some(app) = facts.running_app {
                return blocked(
                    Risk::Low,
                    Certainty::High,
                    Eligibility::NotNow,
                    WhyNot::AppRunning,
                    vec![ok(E::InCaches), warn(E::AppRunning).with_app(app)],
                );
            }
            if bundle_like(facts.name) {
                eligible(
                    Risk::Low,
                    Certainty::High,
                    vec![ok(E::InCaches), ok(E::AppNotRunning)],
                )
            } else {
                eligible(
                    Risk::Low,
                    Certainty::Medium,
                    vec![ok(E::InCaches), info(E::NoAppMatch)],
                )
            }
        }
        Kind::CachesPackage => eligible(
            Risk::Low,
            Certainty::High,
            vec![ok(E::PackageCache), info(E::RefetchedOnInstall)],
        ),
        Kind::Catalog(id) => assess_catalog(id, facts),
        Kind::Log | Kind::CrashReport => eligible(
            Risk::Low,
            Certainty::High,
            vec![
                ok(E::LogOlderThan).with_value(facts.idle_days),
                info(E::NotWrittenSince),
            ],
        ),
        Kind::Temp => eligible(
            Risk::Low,
            Certainty::Medium,
            vec![
                ok(E::TempOlderThan).with_value(facts.idle_days),
                info(E::NotWrittenSince),
            ],
        ),
        Kind::DerivedData => {
            if facts.xcode_running {
                return blocked(
                    Risk::Low,
                    Certainty::High,
                    Eligibility::NotNow,
                    WhyNot::XcodeRunning,
                    vec![ok(E::InDerivedData), warn(E::XcodeRunning)],
                );
            }
            eligible(
                Risk::Low,
                Certainty::High,
                vec![
                    ok(E::InDerivedData),
                    ok(E::XcodeNotRunning),
                    info(E::RebuildSlower),
                ],
            )
        }
        Kind::Archive => eligible(
            Risk::Medium,
            Certainty::High,
            vec![ok(E::Archive), warn(E::LosesDebugSymbols)],
        ),
        Kind::DeviceSupport => eligible(
            Risk::Low,
            Certainty::High,
            vec![ok(E::DeviceSupport), info(E::CopiedAgainOnConnect)],
        ),
        Kind::SimulatorCache => {
            if facts.simulator_running {
                return blocked(
                    Risk::Low,
                    Certainty::High,
                    Eligibility::NotNow,
                    WhyNot::SimulatorRunning,
                    vec![ok(E::SimulatorCache), warn(E::SimulatorRunning)],
                );
            }
            eligible(
                Risk::Low,
                Certainty::High,
                vec![ok(E::SimulatorCache), info(E::RebuildSlower)],
            )
        }
        Kind::SimulatorDevice => blocked(
            Risk::Medium,
            Certainty::High,
            Eligibility::InspectOnly,
            WhyNot::ManageInXcode,
            vec![warn(E::SimulatorData)],
        ),
        Kind::BuildFolder { name } => assess_build(name, facts),
        Kind::LargeFile => blocked(
            Risk::Unknown,
            Certainty::Unknown,
            Eligibility::InspectOnly,
            WhyNot::UserFile,
            vec![
                info(E::LargerThan).with_value(facts.larger_than),
                warn(E::NotKnownCache),
            ],
        ),
        Kind::OldFile => blocked(
            Risk::Unknown,
            Certainty::Unknown,
            Eligibility::InspectOnly,
            WhyNot::UserFile,
            vec![
                info(E::NotOpenedFor).with_value(facts.idle_days),
                warn(E::NotKnownCache),
            ],
        ),
        Kind::AppLeftover { .. } => {
            if let Some(app) = facts.running_app {
                return blocked(
                    Risk::Low,
                    Certainty::High,
                    Eligibility::NotNow,
                    WhyNot::AppRunning,
                    vec![warn(E::AppRunning).with_app(app)],
                );
            }
            if facts.owner_removed {
                eligible(
                    Risk::Low,
                    Certainty::High,
                    vec![ok(E::LeftoverOfRemovedApp), warn(E::LosesAppData)],
                )
            } else {
                // Matched by name to an app that isn't installed: likely, not certain.
                eligible(
                    Risk::Medium,
                    Certainty::Medium,
                    vec![ok(E::LeftoverNoApp), warn(E::LosesAppData)],
                )
            }
        }
        Kind::Installer => eligible(
            Risk::Low,
            Certainty::High,
            vec![ok(E::InstallerFile), info(E::DownloadAgain)],
        ),
        // `project_active` is whether anything inside is open in an app now.
        Kind::UserChosen if facts.project_active => blocked(
            Risk::Medium,
            Certainty::Unknown,
            Eligibility::NotNow,
            WhyNot::OpenInApp,
            vec![warn(E::ChosenByYou), warn(E::OpenInApp)],
        ),
        Kind::UserChosen => eligible(
            Risk::Medium,
            Certainty::Unknown,
            vec![warn(E::ChosenByYou), ok(E::NothingOpen)],
        ),
    }
}

fn assess_catalog(id: &str, facts: &Facts<'_>) -> Assessment {
    use EvidenceCode as E;
    match id {
        "simulatorDevices" | "simulatorRuntimes" => blocked(
            Risk::Medium,
            Certainty::High,
            Eligibility::InspectOnly,
            WhyNot::ManageInXcode,
            vec![warn(E::SimulatorData)],
        ),
        "xcodeDerivedData" => assess_kind(&Kind::DerivedData, facts),
        "xcodeArchives" => assess_kind(&Kind::Archive, facts),
        "xcodeDeviceSupport" => assess_kind(&Kind::DeviceSupport, facts),
        "pnpmStore" => eligible(
            catalog_risk(id),
            Certainty::High,
            vec![ok(E::PackageCache), warn(E::SharedStore)],
        ),
        "rustup" => eligible(
            catalog_risk(id),
            Certainty::High,
            vec![ok(E::Toolchain), warn(E::ReinstallNeeded)],
        ),
        _ => eligible(
            catalog_risk(id),
            Certainty::High,
            vec![ok(E::PackageCache), info(E::RefetchedOnInstall)],
        ),
    }
}

fn assess_build(name: &str, facts: &Facts<'_>) -> Assessment {
    use EvidenceCode as E;
    if facts.project_active {
        return blocked(
            Risk::Medium,
            Certainty::High,
            Eligibility::NotNow,
            WhyNot::ActiveProject,
            vec![ok(E::BuildOutput), warn(E::ProjectActive)],
        );
    }
    match name {
        "node_modules" if facts.lockfile => eligible(
            Risk::Low,
            Certainty::High,
            vec![
                ok(E::LockfileFound),
                ok(E::ProjectInactive),
                info(E::ReinstallNeeded),
            ],
        ),
        "node_modules" => blocked(
            Risk::Medium,
            Certainty::Medium,
            Eligibility::NeedsCheck,
            WhyNot::NoLockfile,
            vec![warn(E::NoLockfile)],
        ),
        // Environments and vendored dependencies come back only after a reinstall; build,
        // dist and out are usually build output but can be checked in.
        ".venv" | "venv" | "vendor" | "Pods" | "build" | "dist" | "out" => eligible(
            Risk::Medium,
            Certainty::Medium,
            vec![
                ok(E::BuildOutput),
                ok(E::ProjectInactive),
                warn(E::ReinstallNeeded),
            ],
        ),
        _ => eligible(
            Risk::Low,
            Certainty::High,
            vec![
                ok(E::BuildOutput),
                ok(E::ProjectInactive),
                info(E::RebuildSlower),
            ],
        ),
    }
}

/// Whether `name` is a build folder a project's tools make (`node_modules`, `.next`, …).
pub fn is_build_folder(name: &str) -> bool {
    BUILD_FOLDERS.contains(&name)
}

/// Build-folder names a project may have, from the Developer page's stacks.
const BUILD_FOLDERS: &[&str] = &[
    "node_modules",
    ".next",
    ".turbo",
    ".nuxt",
    ".svelte-kit",
    ".parcel-cache",
    "dist",
    "build",
    "out",
    "target",
    ".gradle",
    ".venv",
    "venv",
    ".pytest_cache",
    ".mypy_cache",
    ".tox",
    "Pods",
    ".dart_tool",
    "vendor",
];

/// Folders inside the home folder that no item may be in or contain, whatever its kind.
const NEVER: &[&str] = &[
    ".git",
    ".ssh",
    ".gnupg",
    "Library/Keychains",
    "Library/Mobile Documents",
    "Library/CloudStorage",
    "Library/Mail",
    "Library/Messages",
    "Library/Containers",
    "Library/Group Containers",
    "Library/Application Support",
];

fn plain(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .skip(1)
            .all(|component| matches!(component, Component::Normal(_)))
}

/// Whether `path` is a place an item of `kind` may be moved from: a plain absolute path,
/// strictly inside the home folder (or the temporary folder for `Temp`), at the fixed
/// location its kind allows and outside every never-touch folder. `catalog` resolves a
/// catalog ID to its locations.
pub fn within_bounds(
    kind: &Kind,
    path: &Path,
    home: &Path,
    temp: Option<&Path>,
    catalog: impl Fn(&str) -> Vec<std::path::PathBuf>,
) -> bool {
    if !plain(path) || !plain(home) {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if let Kind::Temp = kind {
        return temp
            .is_some_and(|temp| plain(temp) && parent == temp && !name.starts_with("com.apple."));
    }
    let Ok(relative) = path.strip_prefix(home) else {
        return false;
    };
    if relative.as_os_str().is_empty() {
        return false;
    }
    // The one way into Application Support and the containers, which are otherwise never
    // touched: an entry directly in a leftover folder, named for the app it belongs to.
    if let Kind::AppLeftover { owner } = kind {
        return in_leftover_folder(path, home) && leftover_matches(name, owner);
    }
    if never_touched(relative) {
        return false;
    }
    let under = |folder: &str| parent == home.join(folder);
    match kind {
        Kind::AppCache => under("Library/Caches") && !name.starts_with("com.apple."),
        Kind::CachesPackage => under("Library/Caches") && CACHES_PACKAGES.contains(&name),
        Kind::Catalog(id) => catalog(id).iter().any(|location| location == path),
        Kind::Log => under("Library/Logs") && name != "DiagnosticReports",
        Kind::CrashReport => under("Library/Logs/DiagnosticReports"),
        Kind::DerivedData => under("Library/Developer/Xcode/DerivedData"),
        Kind::Archive => under("Library/Developer/Xcode/Archives"),
        Kind::DeviceSupport => {
            parent.parent() == Some(home.join("Library/Developer/Xcode").as_path())
                && parent
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|folder| folder.ends_with(" DeviceSupport"))
        }
        Kind::SimulatorCache => path == home.join("Library/Developer/CoreSimulator/Caches"),
        Kind::BuildFolder { name: expected } => {
            expected == name
                && BUILD_FOLDERS.contains(&name)
                && !relative.starts_with("Library")
                && !relative.starts_with(".Trash")
        }
        Kind::Installer => (under("Downloads") || under("Desktop")) && is_installer_name(name),
        Kind::UserChosen => chosen_in_bounds(relative),
        // Never moved, so never in bounds.
        Kind::SimulatorDevice | Kind::LargeFile | Kind::OldFile => false,
        Kind::Temp | Kind::AppLeftover { .. } => false,
    }
}

/// Whether a build folder outside the home folder (in a folder added through the picker) is
/// in bounds: a known build-folder name, plain path, no `.git` component, strictly inside an
/// added folder that is itself a plain path and not the file-system root.
pub fn build_folder_in_added(
    path: &Path,
    name: &str,
    home: &Path,
    added: &[std::path::PathBuf],
) -> bool {
    plain(path)
        && BUILD_FOLDERS.contains(&name)
        && path.file_name().and_then(|leaf| leaf.to_str()) == Some(name)
        && !path.components().any(|part| part.as_os_str() == ".git")
        // An added folder inside the home folder gets the same protection as the home folder
        // itself: nothing in a never-touch folder, Library or the Trash.
        && path.strip_prefix(home).map_or(true, |relative| {
            !never_touched(relative) && !relative.starts_with("Library") && !relative.starts_with(".Trash")
        })
        && added
            .iter()
            .any(|root| {
                plain(root) && root.parent().is_some() && path.starts_with(root) && path != root
            })
}

/// The Developer catalog's locations for a catalog ID, resolved against the home folder.
pub fn catalog_paths(home: &Path, id: &str) -> Vec<std::path::PathBuf> {
    crate::developer::catalog::CACHES
        .iter()
        .filter(|cache| cache.id == id)
        .flat_map(|cache| cache.paths.iter())
        .filter_map(|location| crate::developer::catalog::resolve(location, Some(home)))
        .collect()
}

/// Whether a stored original path is a place an item of the recorded category could have been
/// moved from: one of the fixed places of a kind in that category (`within_bounds`). Restore
/// and Show in Finder use it so a tampered history can't send an item, or point Finder,
/// anywhere else. Large and old files are never moved, so they never qualify. A build folder
/// outside the home folder qualifies only inside a folder in `added`, which the caller takes
/// from the user's choices in the folder panel, never from the history; the same rule as at
/// approval.
pub fn restorable(
    category: &str,
    path: &Path,
    added: &[std::path::PathBuf],
    home: &Path,
    temp: Option<&Path>,
) -> bool {
    let catalog = |id: &str| catalog_paths(home, id);
    let in_bounds = |kind: &Kind| within_bounds(kind, path, home, temp, catalog);
    let build_folder = |name: &str| {
        in_bounds(&Kind::BuildFolder {
            name: name.to_owned(),
        }) || build_folder_in_added(path, name, home, added)
    };
    let leaf = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let catalog_kinds = |xcode: bool| {
        crate::developer::catalog::CACHES
            .iter()
            .filter(|cache| catalog_is_xcode(cache.id) == xcode)
            .any(|cache| in_bounds(&Kind::Catalog(cache.id)))
    };
    match category {
        "cache" => in_bounds(&Kind::AppCache),
        "pkg" => in_bounds(&Kind::CachesPackage) || catalog_kinds(false),
        "xcode" => {
            [
                Kind::DerivedData,
                Kind::Archive,
                Kind::DeviceSupport,
                Kind::SimulatorCache,
            ]
            .iter()
            .any(in_bounds)
                || catalog_kinds(true)
        }
        "node" => build_folder("node_modules"),
        "build" => leaf != "node_modules" && build_folder(leaf),
        "logs" => [Kind::Log, Kind::CrashReport, Kind::Temp]
            .iter()
            .any(in_bounds),
        // The app it belonged to isn't recorded: any entry directly in a leftover folder.
        "apps" => in_leftover_folder(path, home) || in_bounds(&Kind::Installer),
        "chosen" => in_bounds(&Kind::UserChosen),
        _ => false,
    }
}

/// Whether a path relative to the home folder is in, or contains, a folder no item may come
/// from.
fn never_touched(relative: &Path) -> bool {
    NEVER.iter().any(|never| relative.starts_with(never))
        || relative.components().any(|part| part.as_os_str() == ".git")
}
