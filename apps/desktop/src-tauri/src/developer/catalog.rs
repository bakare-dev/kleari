//! The developer caches and toolchain folders the Developer page knows, and their versions.
//! Versions come from files the tools install (Xcode's `Info.plist`, npm's `package.json`),
//! read without following symlinks; no tool is run. A tool without a readable version file
//! shows its location instead.

use std::ffi::CString;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::measure::open_absolute;
use crate::system_monitor::non_local_mounts;

const MAX_VERSION_FILE: u64 = 256 * 1024;

/// One known cache: its stable ID (the frontend has its name and explanation) and where it
/// lives. A leading `~/` is the home folder.
pub struct Cache {
    pub id: &'static str,
    pub paths: &'static [&'static str],
    pub version: Option<Version>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    Xcode,
    Npm,
}

pub const CACHES: &[Cache] = &[
    Cache {
        id: "xcodeDerivedData",
        paths: &["~/Library/Developer/Xcode/DerivedData"],
        version: Some(Version::Xcode),
    },
    Cache {
        id: "xcodeArchives",
        paths: &["~/Library/Developer/Xcode/Archives"],
        version: Some(Version::Xcode),
    },
    Cache {
        id: "xcodeDeviceSupport",
        paths: &["~/Library/Developer/Xcode/iOS DeviceSupport"],
        version: Some(Version::Xcode),
    },
    Cache {
        id: "simulatorDevices",
        paths: &["~/Library/Developer/CoreSimulator/Devices"],
        version: Some(Version::Xcode),
    },
    Cache {
        id: "simulatorRuntimes",
        paths: &["/Library/Developer/CoreSimulator/Images"],
        version: Some(Version::Xcode),
    },
    Cache {
        id: "homebrew",
        paths: &["~/Library/Caches/Homebrew"],
        version: None,
    },
    Cache {
        id: "npm",
        paths: &["~/.npm/_cacache"],
        version: Some(Version::Npm),
    },
    Cache {
        id: "pnpmStore",
        paths: &["~/Library/pnpm/store"],
        version: None,
    },
    Cache {
        id: "pnpmCache",
        paths: &["~/Library/Caches/pnpm"],
        version: None,
    },
    Cache {
        id: "yarn",
        paths: &["~/Library/Caches/Yarn"],
        version: None,
    },
    Cache {
        id: "bun",
        paths: &["~/.bun/install/cache"],
        version: None,
    },
    Cache {
        id: "gradle",
        paths: &["~/.gradle/caches"],
        version: None,
    },
    Cache {
        id: "maven",
        paths: &["~/.m2/repository"],
        version: None,
    },
    Cache {
        id: "cocoapods",
        paths: &["~/Library/Caches/CocoaPods"],
        version: None,
    },
    Cache {
        id: "pip",
        paths: &["~/Library/Caches/pip"],
        version: None,
    },
    Cache {
        id: "cargo",
        paths: &["~/.cargo/registry"],
        version: None,
    },
    Cache {
        id: "rustup",
        paths: &["~/.rustup/toolchains"],
        version: None,
    },
    Cache {
        id: "goModules",
        paths: &["~/go/pkg/mod"],
        version: None,
    },
    Cache {
        id: "goBuild",
        paths: &["~/Library/Caches/go-build"],
        version: None,
    },
];

/// A catalog path made absolute; `None` for `~/` paths when there is no home folder.
pub fn resolve(path: &str, home: Option<&Path>) -> Option<PathBuf> {
    match path.strip_prefix("~/") {
        Some(rest) => home.map(|home| home.join(rest)),
        None => Some(PathBuf::from(path)),
    }
}

/// Reads a small file at `folder/name` without following symlinks anywhere on the path, and
/// never under a non-local mount (nothing if the mount table can't be read).
fn read_small(folder: &Path, name: &str) -> Option<Vec<u8>> {
    let mounts = non_local_mounts()?;
    if mounts.iter().any(|mount| folder.starts_with(mount)) {
        return None;
    }
    let (dir, dev) = open_absolute(folder)?;
    let file = dir.open_file_child(&CString::new(name).ok()?).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.dev() != dev || metadata.len() > MAX_VERSION_FILE {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_VERSION_FILE).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// A version string worth showing: short, printable, starting with a digit.
fn plausible(version: &str) -> Option<String> {
    let version = version.trim();
    (version.len() <= 40
        && version.starts_with(|c: char| c.is_ascii_digit())
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')))
    .then(|| version.to_owned())
}

pub fn parse_plist_version(bytes: &[u8]) -> Option<String> {
    let value = plist::Value::from_reader(std::io::Cursor::new(bytes)).ok()?;
    plausible(
        value
            .as_dictionary()?
            .get("CFBundleShortVersionString")?
            .as_string()?,
    )
}

pub fn parse_package_version(bytes: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    plausible(value.get("version")?.as_str()?)
}

/// Where npm installs itself with Homebrew or the Node.js installer.
const NPM_FOLDERS: [&str; 2] = [
    "/opt/homebrew/lib/node_modules/npm",
    "/usr/local/lib/node_modules/npm",
];

pub fn read_version(version: Version) -> Option<String> {
    match version {
        Version::Xcode => parse_plist_version(&read_small(
            Path::new("/Applications/Xcode.app/Contents"),
            "Info.plist",
        )?),
        Version::Npm => NPM_FOLDERS.iter().find_map(|folder| {
            parse_package_version(&read_small(Path::new(folder), "package.json")?)
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_from_plists_and_package_files() {
        let plist = br#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleShortVersionString</key><string>17.0</string></dict></plist>"#;
        assert_eq!(parse_plist_version(plist), Some("17.0".to_owned()));
        assert_eq!(
            parse_package_version(br#"{"name":"npm","version":"10.9.2"}"#),
            Some("10.9.2".to_owned())
        );
        // Implausible values are dropped rather than shown.
        assert_eq!(parse_package_version(br#"{"version":"<script>"}"#), None);
        assert_eq!(parse_package_version(br#"{"version":"latest"}"#), None);
        assert_eq!(parse_package_version(b"not json"), None);
    }

    #[test]
    fn catalog_paths_resolve_against_the_home_folder() {
        let home = Path::new("/Users/someone");
        assert_eq!(
            resolve("~/.npm/_cacache", Some(home)),
            Some(PathBuf::from("/Users/someone/.npm/_cacache"))
        );
        assert_eq!(
            resolve("/Library/Developer/CoreSimulator/Images", None),
            Some(PathBuf::from("/Library/Developer/CoreSimulator/Images"))
        );
        assert_eq!(resolve("~/x", None), None);
        let mut ids: Vec<&str> = CACHES.iter().map(|cache| cache.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CACHES.len(), "IDs are unique");
    }

    #[test]
    fn version_files_are_read_without_following_symlinks() {
        let dir = std::env::temp_dir().join(format!("kleari-version-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let dir = std::fs::canonicalize(&dir).expect("canonical");
        std::fs::write(dir.join("real.json"), br#"{"version":"1.2.3"}"#).expect("file");
        std::os::unix::fs::symlink(dir.join("real.json"), dir.join("package.json"))
            .expect("symlink");
        assert_eq!(read_small(&dir, "package.json"), None);
        assert_eq!(
            read_small(&dir, "real.json").and_then(|bytes| parse_package_version(&bytes)),
            Some("1.2.3".to_owned())
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
