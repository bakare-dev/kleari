//! Small pictures for list rows and previews, made on this Mac: an image file's thumbnail by
//! macOS's ImageIO (usually from the preview the file already holds, so even large photos are
//! quick), and an app's or file's icon as Finder shows it. Nothing is kept on disk; the page
//! asks only for rows it shows.

use std::path::Path;

/// What kind of picture to make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The image file itself, scaled down (JPEG).
    Image,
    /// Its icon as Finder shows it: an app's own icon, or a document's (PNG, transparent).
    Icon,
}

/// The longest side asked for, at most: a large preview, not the full photo.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const MAX_PIXELS: u32 = 1600;

/// A picture of `path` at most `max` pixels on its long side, as bytes and their media type;
/// `None` if macOS can't make one.
#[cfg(target_os = "macos")]
pub fn picture(path: &Path, kind: Kind, max: u32) -> Option<(Vec<u8>, &'static str)> {
    let max = max.clamp(16, MAX_PIXELS);
    objc2::rc::autoreleasepool(|_| {
        let image = match kind {
            Kind::Image => macos::image_thumbnail(path, max)?,
            Kind::Icon => macos::icon(path, max)?,
        };
        let (format, media) = match kind {
            Kind::Image => ("public.jpeg", "image/jpeg"),
            Kind::Icon => ("public.png", "image/png"),
        };
        Some((macos::encode(&image, format)?, media))
    })
}

#[cfg(not(target_os = "macos"))]
pub fn picture(_path: &Path, _kind: Kind, _max: u32) -> Option<(Vec<u8>, &'static str)> {
    None
}

/// The image at `path` decoded small by ImageIO, for Similar Images' comparison.
#[cfg(target_os = "macos")]
pub(crate) fn small_image(
    path: &Path,
    max: u32,
) -> Option<objc2_core_foundation::CFRetained<objc2_core_graphics::CGImage>> {
    macos::image_thumbnail(path, max)
}

/// `bytes` as a `data:` URL the page can show (its content security policy allows them).
pub fn data_url(bytes: &[u8], media: &str) -> String {
    format!("data:{media};base64,{}", base64(bytes))
}

/// Standard base64 (RFC 4648, with padding).
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                out.push(char::from(
                    ALPHABET[usize::try_from((triple >> shift) & 63).unwrap_or(0)],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(target_os = "macos")]
mod macos {
    use std::path::Path;
    use std::ptr::NonNull;

    use objc2_app_kit::NSWorkspace;
    use objc2_core_foundation::{
        CFBoolean, CFDictionary, CFMutableData, CFNumber, CFRetained, CFString, CFType, CGPoint,
        CGRect, CGSize, CFURL,
    };
    use objc2_core_graphics::CGImage;
    use objc2_foundation::NSString;

    #[link(name = "ImageIO", kind = "framework")]
    extern "C" {
        fn CGImageSourceCreateWithURL(
            url: &CFURL,
            options: Option<&CFDictionary>,
        ) -> Option<NonNull<CFType>>;
        fn CGImageSourceCreateThumbnailAtIndex(
            source: &CFType,
            index: usize,
            options: Option<&CFDictionary>,
        ) -> Option<NonNull<CGImage>>;
        fn CGImageDestinationCreateWithData(
            data: &CFMutableData,
            kind: &CFString,
            count: usize,
            options: Option<&CFDictionary>,
        ) -> Option<NonNull<CFType>>;
        fn CGImageDestinationAddImage(
            destination: &CFType,
            image: &CGImage,
            properties: Option<&CFDictionary>,
        );
        fn CGImageDestinationFinalize(destination: &CFType) -> bool;

        static kCGImageSourceCreateThumbnailFromImageIfAbsent: Option<&'static CFString>;
        static kCGImageSourceCreateThumbnailWithTransform: Option<&'static CFString>;
        static kCGImageSourceThumbnailMaxPixelSize: Option<&'static CFString>;
        static kCGImageSourceShouldCache: Option<&'static CFString>;
        static kCGImageDestinationLossyCompressionQuality: Option<&'static CFString>;
    }

    fn owned<T: objc2_core_foundation::Type>(value: Option<NonNull<T>>) -> Option<CFRetained<T>> {
        // SAFETY: every function passed here follows the Create rule: an owned object.
        value.map(|value| unsafe { CFRetained::from_raw(value) })
    }

    /// The image file's thumbnail at most `max` pixels on its long side, upright (its
    /// orientation applied), made from its embedded preview when big enough, otherwise from
    /// the image. Nothing is cached by ImageIO.
    pub fn image_thumbnail(path: &Path, max: u32) -> Option<CFRetained<CGImage>> {
        let url = CFURL::from_file_path(path)?;
        // SAFETY: plain reads of ImageIO's constant keys.
        let (if_absent, transform, size, cache) = unsafe {
            (
                kCGImageSourceCreateThumbnailFromImageIfAbsent?,
                kCGImageSourceCreateThumbnailWithTransform?,
                kCGImageSourceThumbnailMaxPixelSize?,
                kCGImageSourceShouldCache?,
            )
        };
        let yes = CFBoolean::new(true);
        let no = CFBoolean::new(false);
        let pixels = CFNumber::new_i32(i32::try_from(max).ok()?);
        let options = CFDictionary::<CFString, CFType>::from_slices(
            &[if_absent, transform, size, cache],
            &[yes.as_ref(), yes.as_ref(), pixels.as_ref(), no.as_ref()],
        );
        let source_options =
            CFDictionary::<CFString, CFType>::from_slices(&[cache], &[no.as_ref()]);
        // SAFETY: `url` and the options are live; the result follows the Create rule.
        let source =
            owned(unsafe { CGImageSourceCreateWithURL(&url, Some(source_options.as_opaque())) })?;
        // SAFETY: `source` is a live image source; index 0 is its first (or only) image.
        owned(unsafe { CGImageSourceCreateThumbnailAtIndex(&source, 0, Some(options.as_opaque())) })
    }

    /// The icon Finder shows for `path`, drawn at `max` × `max` pixels.
    pub fn icon(path: &Path, max: u32) -> Option<CFRetained<CGImage>> {
        let text = path.to_str()?;
        let image = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(text));
        let side = f64::from(max);
        let mut rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(side, side));
        // SAFETY: `rect` is a valid, writable rect for the size wanted; no context or hints.
        let cg = unsafe { image.CGImageForProposedRect_context_hints(&mut rect, None, None) }?;
        // Held as a Core Foundation object like ImageIO's: one more retain, the original's
        // released when it is dropped here.
        // SAFETY: `cg` is a live CGImage.
        Some(unsafe { CFRetained::retain(NonNull::from(&*cg)) })
    }

    /// `image` encoded as `format` (a type identifier such as `public.jpeg`).
    pub fn encode(image: &CGImage, format: &str) -> Option<Vec<u8>> {
        let data = CFMutableData::new(None, 0)?;
        let kind = CFString::from_str(format);
        // SAFETY: `data` and `kind` are live; the result follows the Create rule.
        let destination =
            owned(unsafe { CGImageDestinationCreateWithData(&data, &kind, 1, None) })?;
        // SAFETY: a plain read of ImageIO's constant key.
        let quality_key = unsafe { kCGImageDestinationLossyCompressionQuality }?;
        let quality = CFNumber::new_f64(0.8);
        let properties =
            CFDictionary::<CFString, CFType>::from_slices(&[quality_key], &[quality.as_ref()]);
        // SAFETY: `destination` and `image` are live; one image, as created for.
        unsafe { CGImageDestinationAddImage(&destination, image, Some(properties.as_opaque())) };
        // SAFETY: `destination` is live and has its one image.
        if !unsafe { CGImageDestinationFinalize(&destination) } {
            return None;
        }
        Some(data.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_becomes_a_data_url() {
        assert_eq!(
            data_url(&[0xff, 0xd8, 0xff], "image/jpeg"),
            "data:image/jpeg;base64,/9j/"
        );
        // RFC 4648's test vectors.
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected, "{input}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_makes_a_small_thumbnail_and_an_icon() {
        // An image this test writes: a 300×200 PNG made with ImageIO itself.
        let dir = std::env::temp_dir().join(format!("kleari-thumb-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let png = dir.join("photo.png");
        let icon = picture(
            Path::new("/System/Applications/Calculator.app"),
            Kind::Icon,
            64,
        );
        let (bytes, media) = icon.expect("Calculator's icon");
        assert_eq!(media, "image/png");
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']));
        // Its icon, saved as a PNG, then read back as an image and made small.
        std::fs::write(&png, &bytes).expect("write");
        let (small, media) = picture(&png, Kind::Image, 32).expect("thumbnail");
        assert_eq!(media, "image/jpeg");
        assert!(small.starts_with(&[0xff, 0xd8]));
        assert!(picture(&dir.join("missing.jpg"), Kind::Image, 32).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
