//! Marketing names for Mac model identifiers and macOS major versions. macOS exposes only the
//! identifier (`hw.model`, for example `Mac16,1`) and the version number, so the names are a
//! bundled table. Unknown identifiers return `None` and the UI shows the identifier instead;
//! the table is never guessed from patterns.

/// "MacBook Pro 14″ (2024)" for `Mac16,1`. Chip variants are not encoded: the chip is shown
/// separately from `machdep.cpu.brand_string`.
pub fn model_name(identifier: &str) -> Option<&'static str> {
    MODELS
        .iter()
        .find(|(id, _)| *id == identifier)
        .map(|(_, name)| *name)
}

/// "Tahoe" for 26. Versions released after this table was written return `None`.
pub fn macos_name(version: &str) -> Option<&'static str> {
    let major: u32 = version.split('.').next()?.parse().ok()?;
    match major {
        14 => Some("Sonoma"),
        15 => Some("Sequoia"),
        26 => Some("Tahoe"),
        _ => None,
    }
}

/// Apple silicon Macs (macOS 14 and later). Sizes use the inch mark, as Apple's own pages do.
const MODELS: &[(&str, &str)] = &[
    ("MacBookAir10,1", "MacBook Air (2020)"),
    ("MacBookPro17,1", "MacBook Pro 13″ (2020)"),
    ("Macmini9,1", "Mac mini (2020)"),
    ("iMac21,1", "iMac 24″ (2021)"),
    ("iMac21,2", "iMac 24″ (2021)"),
    ("MacBookPro18,1", "MacBook Pro 16″ (2021)"),
    ("MacBookPro18,2", "MacBook Pro 16″ (2021)"),
    ("MacBookPro18,3", "MacBook Pro 14″ (2021)"),
    ("MacBookPro18,4", "MacBook Pro 14″ (2021)"),
    ("Mac13,1", "Mac Studio (2022)"),
    ("Mac13,2", "Mac Studio (2022)"),
    ("Mac14,2", "MacBook Air 13″ (2022)"),
    ("Mac14,7", "MacBook Pro 13″ (2022)"),
    ("Mac14,3", "Mac mini (2023)"),
    ("Mac14,12", "Mac mini (2023)"),
    ("Mac14,5", "MacBook Pro 14″ (2023)"),
    ("Mac14,9", "MacBook Pro 14″ (2023)"),
    ("Mac14,6", "MacBook Pro 16″ (2023)"),
    ("Mac14,10", "MacBook Pro 16″ (2023)"),
    ("Mac14,15", "MacBook Air 15″ (2023)"),
    ("Mac14,13", "Mac Studio (2023)"),
    ("Mac14,14", "Mac Studio (2023)"),
    ("Mac14,8", "Mac Pro (2023)"),
    ("Mac15,3", "MacBook Pro 14″ (2023)"),
    ("Mac15,4", "iMac 24″ (2023)"),
    ("Mac15,5", "iMac 24″ (2023)"),
    ("Mac15,6", "MacBook Pro 14″ (2023)"),
    ("Mac15,8", "MacBook Pro 14″ (2023)"),
    ("Mac15,10", "MacBook Pro 14″ (2023)"),
    ("Mac15,7", "MacBook Pro 16″ (2023)"),
    ("Mac15,9", "MacBook Pro 16″ (2023)"),
    ("Mac15,11", "MacBook Pro 16″ (2023)"),
    ("Mac15,12", "MacBook Air 13″ (2024)"),
    ("Mac15,13", "MacBook Air 15″ (2024)"),
    ("Mac16,1", "MacBook Pro 14″ (2024)"),
    ("Mac16,6", "MacBook Pro 14″ (2024)"),
    ("Mac16,8", "MacBook Pro 14″ (2024)"),
    ("Mac16,5", "MacBook Pro 16″ (2024)"),
    ("Mac16,7", "MacBook Pro 16″ (2024)"),
    ("Mac16,2", "iMac 24″ (2024)"),
    ("Mac16,3", "iMac 24″ (2024)"),
    ("Mac16,10", "Mac mini (2024)"),
    ("Mac16,11", "Mac mini (2024)"),
    ("Mac16,12", "MacBook Air 13″ (2025)"),
    ("Mac16,13", "MacBook Air 15″ (2025)"),
];
