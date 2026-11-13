use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use super::*;

const DAY: Duration = Duration::from_secs(86_400);

struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("kleari-files-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("home");
        Self(fs::canonicalize(&root).expect("real path"))
    }

    fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("folders");
        fs::write(&path, bytes).expect("write");
        path
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_millis(),
    )
    .expect("fits")
}

fn options() -> Options {
    Options {
        large_min_mb: 100,
        old_days: 180,
    }
}

fn run(home: &Home, state: &FilesState, tool: Tool, options: Options) -> FilesScan {
    let cancel = AtomicBool::new(false);
    scan(state, tool, options, &home.0, &[], now_ms(), &cancel)
}

fn names(group: &Group) -> Vec<&str> {
    group.items.iter().map(|item| item.name.as_str()).collect()
}

/// Sets when `path` was last opened and changed to `ago` before now.
fn age(path: &Path, ago: Duration) {
    let when = SystemTime::now() - ago;
    let file = fs::File::options().write(true).open(path).expect("open");
    file.set_times(fs::FileTimes::new().set_accessed(when).set_modified(when))
        .expect("set times");
}

#[test]
fn copies_are_found_by_content_and_one_is_always_kept() {
    let home = Home::new("copies");
    let content = vec![7_u8; 2_000_000];
    let mut other = content.clone();
    // Same size and same first 64 KB, different further in: not a copy.
    other[1_500_000] = 8;
    let original = home.write("Documents/report.pdf", &content);
    age(&original, 30 * DAY);
    home.write("Desktop/report copy.pdf", &content);
    home.write("Downloads/report (1).pdf", &content);
    home.write("Downloads/almost.pdf", &other);
    // Hidden, in the Library or inside a package: not walked.
    home.write("Library/Caches/report.pdf", &content);
    home.write(".secrets/report.pdf", &content);
    home.write("Applications/Tool.app/Contents/report.pdf", &content);
    let state = FilesState::default();

    let result = run(&home, &state, Tool::Duplicates, options());
    assert_eq!(result.groups.len(), 1);
    let copies = &result.groups[0];
    assert_eq!(copies.kind, GroupKind::Copies);
    // The oldest copy comes first: the one kept by default.
    assert_eq!(
        names(copies),
        ["report.pdf", "report copy.pdf", "report (1).pdf"]
    );
    assert!(copies.reclaimable >= 4_000_000);

    // Asking for every copy still keeps the first one.
    let ids: Vec<String> = copies.items.iter().map(|item| item.id.clone()).collect();
    let chosen = state.chosen(&ids);
    assert_eq!(chosen.len(), 2);
    assert!(chosen.iter().all(|item| item.path != original));
    // Asking for some of them sends exactly those.
    let chosen = state.chosen(&ids[..1]);
    assert_eq!(chosen.len(), 1);
    assert_eq!(chosen[0].path, original);
}

#[test]
fn large_and_old_files_are_listed_in_their_own_groups() {
    let home = Home::new("large-old");
    let big = vec![1_u8; 101_000_000];
    home.write("Movies/film.mov", &big);
    let old = home.write("Documents/archive.zip", &vec![2_u8; 60_000_000]);
    age(&old, 400 * DAY);
    home.write("Documents/recent.zip", &vec![3_u8; 60_000_000]);
    let state = FilesState::default();

    let result = run(&home, &state, Tool::LargeOld, options());
    let large = result
        .groups
        .iter()
        .find(|group| group.kind == GroupKind::Large)
        .expect("large");
    let old_group = result
        .groups
        .iter()
        .find(|group| group.kind == GroupKind::Old)
        .expect("old");
    assert_eq!(names(large), ["film.mov"]);
    assert_eq!(large.items[0].file_category, Some(FileCategory::Video));
    assert!(large.items[0].selectable);
    assert_eq!(names(old_group), ["archive.zip"]);
    assert!(result.files_seen >= 3);
}

#[cfg(target_os = "macos")]
#[test]
fn downloads_are_grouped_by_the_app_that_downloaded_them() {
    let home = Home::new("downloads");
    let tag = |path: &Path, value: &str| {
        let path = CString::new(path.as_os_str().as_bytes()).expect("path");
        // SAFETY: NUL-terminated name and path; `value` is readable for its length.
        let status = unsafe {
            libc::setxattr(
                path.as_ptr(),
                c"com.apple.quarantine".as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        assert_eq!(status, 0, "tagged");
    };
    let safari = home.write("Downloads/page.pdf", &[1; 20_000]);
    tag(
        &safari,
        "0083;65f0a1b2;Safari;8C1E7D2A-0000-4000-8000-000000000001",
    );
    let slack = home.write("Downloads/design.png", &[1; 30_000]);
    tag(
        &slack,
        "0083;65f0a1b2;Slack;8C1E7D2A-0000-4000-8000-000000000002",
    );
    home.write("Downloads/untagged.txt", &[1; 10_000]);
    let state = FilesState::default();

    let result = run(&home, &state, Tool::Downloads, options());
    let mut labels: Vec<(&str, Vec<&str>)> = result
        .groups
        .iter()
        .map(|group| (group.label.as_str(), names(group)))
        .collect();
    labels.sort();
    assert_eq!(
        labels,
        [
            ("Safari", vec!["page.pdf"]),
            ("Slack", vec!["design.png"]),
            ("Unknown", vec!["untagged.txt"]),
        ]
    );
}

#[test]
fn ai_models_caches_and_logs_are_found_in_known_places() {
    let home = Home::new("ai");
    home.write(".ollama/models/blobs/sha256-abc", &[1; 300_000]);
    home.write(
        ".cache/huggingface/hub/models--org--small/blob",
        &[1; 200_000],
    );
    home.write(
        ".cache/huggingface/hub/models--org--large/blob",
        &[1; 400_000],
    );
    home.write("Library/Logs/Claude/main.log", &[1; 10_000]);
    let state = FilesState::default();

    let result = run(&home, &state, Tool::Ai, options());
    let by = |label: &str| {
        result
            .groups
            .iter()
            .find(|group| group.label == label)
            .unwrap_or_else(|| panic!("{label} found"))
    };
    // Each Hugging Face model is its own row; Ollama is one, with how to remove a single model.
    assert_eq!(
        names(by("Hugging Face")),
        ["models--org--large", "models--org--small"]
    );
    assert_eq!(names(by("Ollama")), ["models"]);
    assert!(by("Ollama")
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("ollama rm")));
    assert_eq!(names(by("Claude")), ["Claude"]);
    // All of them can be chosen.
    assert!(result
        .groups
        .iter()
        .flat_map(|group| &group.items)
        .all(|item| item.selectable));
}

#[test]
fn hashes_close_together_are_grouped_and_distant_ones_are_not() {
    assert_eq!(images::difference_hash(&[0; 72]), 0);
    let mut rising = [0_u8; 72];
    for (index, pixel) in rising.iter_mut().enumerate() {
        *pixel = u8::try_from(index % 9).expect("small") * 20;
    }
    assert_eq!(images::difference_hash(&rising), u64::MAX);
    let hashes = [0b1111, 0b1110, u64::MAX, 0b0111_1111, u64::MAX - 1];
    let mut groups = images::cluster(&hashes);
    for group in &mut groups {
        group.sort_unstable();
    }
    groups.sort();
    // 0b1111, 0b1110 and 0b0111_1111 are within 5 bits; the two near-all-ones are together.
    assert_eq!(groups, [vec![0, 1, 3], vec![2, 4]]);
}

/// An uncompressed 8-bit grayscale TIFF, which macOS decodes (only macOS tests use it).
#[cfg(target_os = "macos")]
fn tiff(width: u16, height: u16, pixel: impl Fn(u16, u16) -> u8) -> Vec<u8> {
    let mut pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            pixels.push(pixel(x, y));
        }
    }
    let data_offset = 8_u32;
    let ifd_offset = data_offset + u32::try_from(pixels.len()).expect("small");
    let mut out = b"II*\0".to_vec();
    out.extend(ifd_offset.to_le_bytes());
    out.extend(&pixels);
    let entries: [(u16, u16, u32, u32); 8] = [
        (256, 3, 1, u32::from(width)),
        (257, 3, 1, u32::from(height)),
        (258, 3, 1, 8),
        (259, 3, 1, 1),
        (262, 3, 1, 1),
        (273, 4, 1, data_offset),
        (278, 3, 1, u32::from(height)),
        (279, 4, 1, u32::try_from(pixels.len()).expect("small")),
    ];
    out.extend(u16::try_from(entries.len()).expect("small").to_le_bytes());
    for (tag, kind, count, value) in entries {
        out.extend(tag.to_le_bytes());
        out.extend(kind.to_le_bytes());
        out.extend(count.to_le_bytes());
        if kind == 3 {
            out.extend(u16::try_from(value).expect("short").to_le_bytes());
            out.extend([0, 0]);
        } else {
            out.extend(value.to_le_bytes());
        }
    }
    out.extend(0_u32.to_le_bytes());
    out
}

#[cfg(target_os = "macos")]
#[test]
fn images_that_look_alike_are_grouped_and_the_largest_is_kept() {
    let home = Home::new("similar");
    // A left-to-right gradient, the same at a different size, and its mirror image.
    let gradient = |size: u16| {
        tiff(size, size, move |x, _| {
            u8::try_from(u32::from(x) * 255 / u32::from(size)).expect("byte")
        })
    };
    home.write("Pictures/shot.tif", &gradient(300));
    home.write("Pictures/shot small.tif", &gradient(240));
    home.write(
        "Pictures/mirror.tif",
        &tiff(300, 300, |x, _| {
            255 - u8::try_from(u32::from(x) * 255 / 300).expect("byte")
        }),
    );
    let state = FilesState::default();

    let result = run(&home, &state, Tool::Similar, options());
    assert_eq!(result.groups.len(), 1, "{:?}", result.groups);
    let similar = &result.groups[0];
    assert_eq!(similar.kind, GroupKind::Similar);
    assert_eq!(names(similar), ["shot.tif", "shot small.tif"]);
    let ids: Vec<String> = similar.items.iter().map(|item| item.id.clone()).collect();
    assert_eq!(state.chosen(&ids).len(), 1, "the largest is kept");
}

#[test]
fn only_the_menus_options_are_accepted() {
    assert!(options().valid());
    assert!(!Options {
        large_min_mb: 1,
        ..options()
    }
    .valid());
    assert!(!Options {
        old_days: 3,
        ..options()
    }
    .valid());
}
