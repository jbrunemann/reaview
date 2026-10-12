//! The file details shown in the status bar.

use std::path::Path;

use objc2_app_kit::NSImage;
use objc2_foundation::{NSByteCountFormatter, NSByteCountFormatterCountStyle, NSString};
use objc2_uniform_type_identifiers::UTType;

/// "JPEG image  ·  4032 × 3024 pixels  ·  2.4 MB"; parts that cannot be
/// determined are left out.
pub fn describe(path: &Path, image: &NSImage) -> String {
    [kind(path), pixel_size(image), file_size(path)]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("  ·  ")
}

/// The localized type name Finder shows in its Kind column, e.g. "PNG image".
fn kind(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy();
    let ty = UTType::typeWithFilenameExtension(&NSString::from_str(&ext))?;
    Some(ty.localizedDescription()?.to_string())
}

/// The largest bitmap among the image's representations: an .icns or .ico
/// holds several sizes, and NSImage's own size is in points, not pixels.
fn pixel_size(image: &NSImage) -> Option<String> {
    let (width, height) = image
        .representations()
        .iter()
        .map(|rep| (rep.pixelsWide(), rep.pixelsHigh()))
        .filter(|&(w, h)| w > 0 && h > 0)
        .max_by_key(|&(w, h)| w * h)?;
    Some(format!("{width} × {height} pixels"))
}

/// Byte count formatted the way Finder does (decimal units).
fn file_size(path: &Path) -> Option<String> {
    let bytes = std::fs::metadata(path).ok()?.len();
    let text = NSByteCountFormatter::stringFromByteCount_countStyle(
        bytes as i64,
        NSByteCountFormatterCountStyle::File,
    );
    Some(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::AllocAnyThread;
    use objc2_foundation::NSURL;

    fn describe_asset(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(name);
        let url = NSURL::from_file_path(&path).unwrap();
        let image = NSImage::initWithContentsOfURL(NSImage::alloc(), &url).unwrap();
        describe(&path, &image)
    }

    #[test]
    fn describes_type_pixels_and_size() {
        let text = describe_asset("AppIcon.png");
        let parts: Vec<&str> = text.split("  ·  ").collect();
        assert_eq!(parts.len(), 3, "{text}");
        assert!(parts[0].contains("PNG"), "{text}");
        assert_eq!(parts[1], "1024 × 1024 pixels");
        assert!(parts[2].ends_with("KB"), "{text}");
    }

    #[test]
    fn uses_largest_representation() {
        assert!(describe_asset("AppIcon.icns").contains("1024 × 1024 pixels"));
    }
}
