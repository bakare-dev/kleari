//! The installed apps, so their data can be counted with them, as macOS's Storage settings
//! do. For each app bundle directly in an Applications folder, its `Contents/Info.plist` is
//! read for the bundle identifier and names. This is the only file content the analysis
//! reads: app metadata, never the user's documents.
//!
//! Every folder and the plist are opened relative to their open parent without following
//! symlinks (`fsdir`); the plist must be a regular file of at most 256 KB and is parsed as
//! untrusted input. Bounded to `MAX_APPS` bundles.

use std::collections::HashSet;
use std::ffi::CString;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::storage::fsdir::DirHandle;

const MAX_APPS: usize = 2_000;
const MAX_PLIST_BYTES: u64 = 256 * 1024;
/// Shorter names (one letter) would match unrelated folders.
const MIN_KEY_CHARS: usize = 2;
/// A vendor name from a bundle identifier must be at least this long to be used.
const MIN_VENDOR_CHARS: usize = 3;
/// Vendors whose names are too general to claim a folder: Apple's own apps keep their data
/// in folders macOS manages, and these appear in many unrelated identifiers.
const IGNORED_VENDORS: [&str; 3] = ["apple", "app", "example"];

/// Lowercased app names, bundle identifiers and vendor names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppIndex {
    keys: HashSet<String>,
    bundle_ids: Vec<String>,
}

impl AppIndex {
    /// `keys` are names and bundle identifiers. A bundle identifier (`com.google.Chrome`) also
    /// adds its vendor (`google`), since apps often keep data in a vendor folder
    /// (`Application Support/Google`, `JetBrains`, `Microsoft`).
    pub fn from_keys<I: IntoIterator<Item = String>>(keys: I) -> Self {
        let mut index = Self::default();
        for key in keys {
            let key = key.trim().to_lowercase();
            if key.chars().count() < MIN_KEY_CHARS {
                continue;
            }
            let parts: Vec<&str> = key.split('.').collect();
            if parts.len() >= 3
                && parts
                    .iter()
                    .all(|part| !part.is_empty() && !part.contains(' '))
            {
                let vendor = parts[1];
                if vendor.chars().count() >= MIN_VENDOR_CHARS && !IGNORED_VENDORS.contains(&vendor)
                {
                    index.keys.insert(vendor.to_owned());
                }
                index.bundle_ids.push(key.clone());
            }
            index.keys.insert(key);
        }
        index
    }

    /// Whether a data folder's name is an installed app's name, bundle identifier or vendor,
    /// or a bundle identifier with a suffix (`com.google.Chrome.helper`).
    pub fn matches(&self, folder: &str) -> bool {
        let folder = folder.to_lowercase();
        self.keys.contains(&folder)
            || self.bundle_ids.iter().any(|id| {
                folder
                    .strip_prefix(id.as_str())
                    .is_some_and(|rest| rest.starts_with('.'))
            })
    }

    /// Reads the app bundles directly inside each folder. Missing folders are skipped.
    /// Reads the app bundles directly inside each folder. Like the main walk, it never leaves
    /// a folder's volume: a bundle, its `Contents` or its plist on another device is skipped.
    /// Nothing at or below a mount in `non_local` (network shares, automounts) is touched,
    /// including the folder itself. Missing folders are skipped.
    ///
    /// `mounts` reads the non-local mount table (`getmntinfo(MNT_NOWAIT)`, no file-system
    /// I/O). It is read again before each folder and each bundle, so a share mounted during
    /// indexing is seen at the next step; if it can't be read, indexing stops there. A volume
    /// mounted between a check and the next lookup is still looked up once, and the device
    /// checks then stop the descent.
    pub fn read(folders: &[&Path], mounts: &dyn Fn() -> Option<Vec<PathBuf>>) -> Self {
        let on_mount =
            |path: &Path, table: &[PathBuf]| table.iter().any(|mount| path.starts_with(mount));
        let mut keys = Vec::new();
        let mut apps = 0;
        'folders: for folder in folders {
            let Some(table) = mounts() else {
                break;
            };
            if on_mount(folder, &table) {
                continue;
            }
            let Ok((mut dir, stat)) = DirHandle::open_root(folder) else {
                continue;
            };
            let disk_dev = stat.dev;
            while let Some(Ok(name)) = dir.next_name() {
                if apps >= MAX_APPS {
                    break;
                }
                let Some(stem) = name
                    .to_str()
                    .ok()
                    .and_then(|name| name.strip_suffix(".app"))
                else {
                    continue;
                };
                let bundle = folder.join(stem).with_extension("app");
                let Some(table) = mounts() else {
                    break 'folders;
                };
                if on_mount(&bundle, &table) {
                    continue;
                }
                // A symlinked app (Safari links into the system) isn't followed, and an app on
                // another volume isn't read.
                if !dir
                    .stat_child(&name)
                    .is_ok_and(|stat| stat.is_dir() && stat.dev == disk_dev)
                {
                    continue;
                }
                apps += 1;
                keys.push(stem.to_owned());
                if !on_mount(&bundle.join("Contents"), &table) {
                    keys.extend(info_keys(&dir, &name, disk_dev).unwrap_or_default());
                }
            }
        }
        Self::from_keys(keys)
    }
}

fn info_keys(applications: &DirHandle, bundle: &CString, disk_dev: u64) -> Option<Vec<String>> {
    use std::os::unix::fs::MetadataExt;

    let (app, opened) = applications.open_child(bundle).ok()?;
    if opened.dev != disk_dev {
        return None;
    }
    let (contents, opened) = app.open_child(&CString::new("Contents").ok()?).ok()?;
    if opened.dev != disk_dev {
        return None;
    }
    let file = contents
        .open_file_child(&CString::new("Info.plist").ok()?)
        .ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.dev() != disk_dev || metadata.len() > MAX_PLIST_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_PLIST_BYTES).read_to_end(&mut bytes).ok()?;
    Some(parse_info(&bytes))
}

/// The bundle identifier and names from an `Info.plist` (XML or binary).
pub fn parse_info(bytes: &[u8]) -> Vec<String> {
    let Ok(value) = plist::Value::from_reader(std::io::Cursor::new(bytes)) else {
        return Vec::new();
    };
    let Some(dictionary) = value.as_dictionary() else {
        return Vec::new();
    };
    ["CFBundleIdentifier", "CFBundleName", "CFBundleDisplayName"]
        .iter()
        .filter_map(|key| dictionary.get(key)?.as_string())
        .filter(|text| text.len() <= 255 && !text.as_bytes().contains(&0))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn plist(id: &str, name: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict>
            <key>CFBundleIdentifier</key><string>{id}</string>
            <key>CFBundleName</key><string>{name}</string></dict></plist>"#
        )
    }

    #[test]
    fn info_plists_give_the_bundle_identifier_and_names() {
        assert_eq!(
            parse_info(plist("com.google.Chrome", "Chrome").as_bytes()),
            ["com.google.Chrome", "Chrome"]
        );
        assert!(parse_info(b"not a plist").is_empty());
        assert!(parse_info(b"<plist><array/></plist>").is_empty());
    }

    #[test]
    fn matching_ignores_case_and_too_short_keys() {
        let index = AppIndex::from_keys([
            "Google Chrome".to_owned(),
            "com.google.Chrome".to_owned(),
            "X".to_owned(),
        ]);
        assert!(index.matches("google chrome"));
        assert!(index.matches("COM.GOOGLE.CHROME"));
        assert!(!index.matches("x"));
    }

    #[test]
    fn vendors_and_suffixed_identifiers_match_but_apple_and_lookalikes_do_not() {
        let index = AppIndex::from_keys([
            "com.google.Chrome".to_owned(),
            "com.jetbrains.goland".to_owned(),
            "com.apple.Safari".to_owned(),
            "Some App".to_owned(),
        ]);
        // Vendor folders, from the bundle identifier.
        assert!(index.matches("Google"));
        assert!(index.matches("JetBrains"));
        // A bundle identifier with a suffix, as caches often use.
        assert!(index.matches("com.google.Chrome.helper"));
        // Apple isn't a vendor key; a lookalike prefix isn't a suffix.
        assert!(!index.matches("Apple"));
        assert!(!index.matches("com.google.Chromecast"));
        assert!(!index.matches("com"));
        // Names with spaces aren't read as identifiers.
        assert!(!index.matches("Some"));
    }

    #[test]
    fn reads_app_bundles_without_following_symlinks() {
        let root = std::env::temp_dir().join(format!("kleari-apps-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let contents = root.join("Applications/Google Chrome.app/Contents");
        fs::create_dir_all(&contents).expect("bundle");
        fs::write(
            contents.join("Info.plist"),
            plist("com.google.Chrome", "Chrome"),
        )
        .expect("plist");
        // A bundle without a plist still counts by its folder name.
        fs::create_dir_all(root.join("Applications/Bare.app")).expect("bare");
        // A symlinked app and an ordinary folder are ignored.
        let elsewhere = root.join("Elsewhere/Linked.app/Contents");
        fs::create_dir_all(&elsewhere).expect("linked");
        fs::write(
            elsewhere.join("Info.plist"),
            plist("com.example.linked", "Linked"),
        )
        .expect("plist");
        std::os::unix::fs::symlink(
            root.join("Elsewhere/Linked.app"),
            root.join("Applications/Linked.app"),
        )
        .expect("symlink");
        fs::create_dir_all(root.join("Applications/Utilities")).expect("folder");
        // A symlinked plist isn't read.
        let sneaky = root.join("Applications/Sneaky.app/Contents");
        fs::create_dir_all(&sneaky).expect("sneaky");
        std::os::unix::fs::symlink(elsewhere.join("Info.plist"), sneaky.join("Info.plist"))
            .expect("symlink");

        let applications = root.join("Applications");
        let none = || Some(Vec::new());
        let index = AppIndex::read(&[&applications, &root.join("Missing")], &none);
        for key in [
            "google chrome",
            "com.google.chrome",
            "chrome",
            "bare",
            "sneaky",
        ] {
            assert!(index.matches(key), "{key}");
        }
        for key in ["linked", "com.example.linked", "utilities"] {
            assert!(!index.matches(key), "{key}");
        }

        // An Applications folder under a non-local mount isn't opened; nor is a single app
        // bundle under one.
        let mounted = |paths: Vec<PathBuf>| move || Some(paths.clone());
        let refused = AppIndex::read(&[&applications], &mounted(vec![applications.clone()]));
        assert_eq!(refused, AppIndex::default());
        let one = AppIndex::read(
            &[&applications],
            &mounted(vec![applications.join("Google Chrome.app")]),
        );
        assert!(!one.matches("google chrome") && !one.matches("com.google.chrome"));
        assert!(one.matches("bare"));
        // Only the Contents folder under a mount: the name counts, the plist isn't read.
        let contents = AppIndex::read(
            &[&applications],
            &mounted(vec![applications.join("Google Chrome.app/Contents")]),
        );
        assert!(contents.matches("google chrome") && !contents.matches("com.google.chrome"));

        // An unreadable mount table stops indexing.
        assert_eq!(
            AppIndex::read(&[&applications], &|| None),
            AppIndex::default()
        );
        // The table is read again before the folder and before every bundle.
        let reads = std::cell::Cell::new(0);
        let counting = || {
            reads.set(reads.get() + 1);
            Some(Vec::new())
        };
        let _ = AppIndex::read(&[&applications], &counting);
        let bundles = fs::read_dir(&applications)
            .expect("list")
            .filter(|entry| {
                entry
                    .as_ref()
                    .is_ok_and(|entry| entry.file_name().to_string_lossy().ends_with(".app"))
            })
            .count();
        assert_eq!(reads.get(), 1 + bundles);
        fs::remove_dir_all(&root).expect("cleanup");
    }
}
