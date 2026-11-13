//! Similar Images: photos and pictures in the home folder that look alike (burst shots, edits,
//! resized copies). Each image is decoded by macOS (ImageIO through `NSImage`, as Quick Look
//! does) and drawn into a 9×8 grayscale thumbnail; its 64-bit difference hash says how it looks.
//! Images whose hashes differ in at most `MAX_DISTANCE` bits are grouped. Pixels never leave
//! the function and nothing is stored.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{file_measured, group, walk, Ctx, FilesScan, GroupKind};
use crate::storage::fsdir::FileStat;

/// Images smaller than this (icons, thumbnails) aren't compared.
const MIN_IMAGE_BYTES: u64 = 50_000;
/// Images compared, at most.
const MAX_IMAGES: usize = 20_000;
/// Hashes this close are "similar". 0 is identical-looking; 64 is unrelated.
pub const MAX_DISTANCE: u32 = 5;
/// Image types macOS decodes that are worth comparing (not icons or vector art).
const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "heic", "heif", "tif", "tiff", "webp", "dng", "cr2", "nef", "arw",
];

/// The difference hash of a 9×8 grayscale thumbnail: bit `8·row + col` is set when the pixel
/// is darker than the one to its right.
pub fn difference_hash(pixels: &[u8; 72]) -> u64 {
    let mut hash = 0_u64;
    for row in 0..8 {
        for col in 0..8 {
            if pixels[row * 9 + col] < pixels[row * 9 + col + 1] {
                hash |= 1 << (row * 8 + col);
            }
        }
    }
    hash
}

/// Groups of indices whose hashes are within `MAX_DISTANCE` of one another (transitively).
pub fn cluster(hashes: &[u64]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..hashes.len()).collect();
    fn root(parent: &mut [usize], mut at: usize) -> usize {
        while parent[at] != at {
            parent[at] = parent[parent[at]];
            at = parent[at];
        }
        at
    }
    for a in 0..hashes.len() {
        for b in (a + 1)..hashes.len() {
            if (hashes[a] ^ hashes[b]).count_ones() <= MAX_DISTANCE {
                let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                if ra != rb {
                    parent[rb] = ra;
                }
            }
        }
    }
    let mut groups: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for index in 0..hashes.len() {
        let top = root(&mut parent, index);
        groups.entry(top).or_default().push(index);
    }
    groups
        .into_values()
        .filter(|members| members.len() > 1)
        .collect()
}

/// The 9×8 grayscale thumbnail of the image at `path`, decoded by macOS. `None` when it can't
/// be decoded.
#[cfg(target_os = "macos")]
pub fn thumbnail(path: &Path) -> Option<[u8; 72]> {
    use objc2::AnyThread;
    use objc2_app_kit::NSImage;
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, CGInterpolationQuality,
    };
    use objc2_foundation::NSURL;

    objc2::rc::autoreleasepool(|_| {
        let url = NSURL::from_file_path(path)?;
        let image = NSImage::initWithContentsOfURL(NSImage::alloc(), &url)?;
        // SAFETY: a null proposed rect asks for the image's own size; no context or hints.
        let cg = unsafe {
            image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None)
        }?;
        let mut pixels = [0_u8; 72];
        let space = CGColorSpace::new_device_gray()?;
        // SAFETY: `pixels` is 9 × 8 bytes, one 8-bit gray byte per pixel with no alpha, and
        // outlives the context, which is dropped at the end of this closure.
        let context = unsafe {
            CGBitmapContextCreate(
                pixels.as_mut_ptr().cast(),
                9,
                8,
                8,
                9,
                Some(&space),
                CGImageAlphaInfo::None.0,
            )
        }?;
        CGContext::set_interpolation_quality(Some(&context), CGInterpolationQuality::High);
        CGContext::draw_image(
            Some(&context),
            CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(9.0, 8.0)),
            Some(&cg),
        );
        drop(context);
        Some(pixels)
    })
}

#[cfg(not(target_os = "macos"))]
pub fn thumbnail(_path: &Path) -> Option<[u8; 72]> {
    None
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| EXTENSIONS.contains(&extension.to_lowercase().as_str()))
}

pub(super) fn similar(
    ctx: &mut Ctx<'_>,
    excluded: &[PathBuf],
    cancel: &AtomicBool,
    result: &mut FilesScan,
) {
    let mut images: Vec<(PathBuf, FileStat)> = Vec::new();
    let home = ctx.home.to_path_buf();
    let mut seen = std::collections::HashSet::new();
    let walked = walk::walk(&home, excluded, cancel, &mut |path, stat| {
        if images.len() < MAX_IMAGES
            && stat.blocks.saturating_mul(512) >= MIN_IMAGE_BYTES
            && is_image(path)
            && seen.insert((stat.dev, stat.ino))
        {
            images.push((path.to_path_buf(), *stat));
        }
    });
    super::record_walk(result, walked);
    let mut hashed: Vec<(PathBuf, FileStat, u64)> = Vec::with_capacity(images.len());
    for (path, stat) in images {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        // Decode only the file the walk saw, not one put in its place since.
        if walk::open_same(&path, &stat).is_none() {
            continue;
        }
        if let Some(pixels) = thumbnail(&path) {
            hashed.push((path, stat, difference_hash(&pixels)));
        }
    }
    let hashes: Vec<u64> = hashed.iter().map(|(_, _, hash)| *hash).collect();
    let mut clusters = cluster(&hashes);
    let bytes_of = |index: &usize| hashed[*index].1.blocks.saturating_mul(512);
    // Most space to gain first: everything but the largest image of each group.
    let reclaimable = |members: &Vec<usize>| {
        let total = members
            .iter()
            .map(bytes_of)
            .fold(0_u64, u64::saturating_add);
        total.saturating_sub(members.iter().map(bytes_of).max().unwrap_or(0))
    };
    clusters.sort_by_key(|members| std::cmp::Reverse(reclaimable(members)));
    clusters.truncate(super::MAX_GROUPS);
    for mut members in clusters {
        // Kept by default: the largest (usually the best quality), first.
        members.sort_by_key(|index| std::cmp::Reverse(bytes_of(index)));
        let saved = reclaimable(&members);
        let group_id = crate::ids::new_id(0);
        let items: Vec<_> = members
            .iter()
            .enumerate()
            .map(|(position, index)| {
                let (path, stat, _) = &hashed[*index];
                ctx.item(
                    path,
                    stat,
                    file_measured(stat),
                    Some((&group_id, position == 0)),
                )
            })
            .collect();
        let mut made = group(GroupKind::Similar, String::new(), None, Vec::new(), saved);
        made.id = group_id;
        made.bytes = items
            .iter()
            .fold(0_u64, |total, item| total.saturating_add(item.bytes));
        made.items = items;
        result.groups.push(made);
    }
}
