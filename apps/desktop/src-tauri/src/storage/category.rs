//! File-type categories for filtering, decided by extension only (contents are never read).

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileCategory {
    Video,
    Image,
    Audio,
    Archive,
    DiskImage,
    Document,
    Code,
    Other,
}

const TABLE: &[(FileCategory, &[&str])] = &[
    (
        FileCategory::Video,
        &[
            "mp4", "mov", "m4v", "mkv", "avi", "webm", "wmv", "mpg", "mpeg", "prores",
        ],
    ),
    (
        FileCategory::Image,
        &[
            "jpg", "jpeg", "png", "gif", "heic", "heif", "tif", "tiff", "raw", "dng", "cr2", "nef",
            "arw", "psd", "webp", "bmp", "svg",
        ],
    ),
    (
        FileCategory::Audio,
        &[
            "mp3", "m4a", "aac", "wav", "aiff", "aif", "flac", "ogg", "opus", "alac",
        ],
    ),
    (
        FileCategory::Archive,
        &[
            "zip", "tar", "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "xip",
        ],
    ),
    (
        FileCategory::DiskImage,
        &[
            "dmg",
            "iso",
            "img",
            "sparseimage",
            "sparsebundle",
            "vmdk",
            "qcow2",
            "vdi",
            "ipsw",
        ],
    ),
    (
        FileCategory::Document,
        &[
            "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "key", "pages", "numbers", "txt",
            "md", "rtf", "csv", "epub",
        ],
    ),
    (
        FileCategory::Code,
        &[
            "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "swift", "c", "h", "cpp",
            "hpp", "m", "rb", "php", "cs", "json", "yaml", "yml", "toml", "lock", "o", "a", "so",
            "dylib", "rlib", "rmeta", "class", "jar", "wasm", "pyc", "map", "pack", "idx",
        ],
    ),
];

pub fn category_of(path: &Path) -> FileCategory {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return FileCategory::Other;
    };
    let extension = extension.to_ascii_lowercase();
    TABLE
        .iter()
        .find(|(_, extensions)| extensions.contains(&extension.as_str()))
        .map_or(FileCategory::Other, |(category, _)| *category)
}
