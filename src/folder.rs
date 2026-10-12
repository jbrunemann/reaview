//! Finding the images that sit next to the current one in its folder.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2_foundation::{NSComparisonResult, NSString};
use objc2_uniform_type_identifiers::{UTType, UTTypeImage};

/// Every image in the folder containing `path`, sorted the way Finder sorts
/// names. Hidden files and anything that is not an image are skipped.
/// The folder is re-read on every call so added or removed files show up.
pub fn sibling_images(path: &Path) -> Vec<PathBuf> {
    let Some(dir) = path.parent() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut images: Vec<(Retained<NSString>, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file() || t.is_symlink()))
        .map(|entry| entry.path())
        .filter(|path| !is_hidden(path) && is_image(path))
        .map(|path| (NSString::from_str(&path.file_name().unwrap().to_string_lossy()), path))
        .collect();
    images.sort_by(|(a, _), (b, _)| finder_order(a, b));
    images.into_iter().map(|(_, path)| path).collect()
}

/// The image `delta` steps away from `current`, or `None` at either end.
/// If `current` has been deleted, stepping starts from where it used to sort.
pub fn neighbour<'a>(siblings: &'a [PathBuf], current: &Path, delta: isize) -> Option<&'a PathBuf> {
    let index = match siblings.iter().position(|p| p == current) {
        Some(index) => index as isize + delta,
        None => {
            let name = file_name(current);
            let insertion = siblings
                .iter()
                .position(|p| finder_order(&file_name(p), &name) == Ordering::Greater)
                .unwrap_or(siblings.len()) as isize;
            if delta > 0 { insertion + delta - 1 } else { insertion + delta }
        }
    };
    usize::try_from(index).ok().and_then(|index| siblings.get(index))
}

fn file_name(path: &Path) -> Retained<NSString> {
    NSString::from_str(&path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default())
}

/// Finder's name ordering: case-insensitive, with numbers compared by value
/// ("img2" before "img10").
fn finder_order(a: &NSString, b: &NSString) -> Ordering {
    match a.localizedStandardCompare(b) {
        NSComparisonResult::Ascending => Ordering::Less,
        NSComparisonResult::Descending => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

fn is_hidden(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))
}

/// Whether the file extension maps to a type conforming to `public.image`,
/// the same family of types Preview.app registers PVImageContainer for.
fn is_image(path: &Path) -> bool {
    let Some(ext) = path.extension() else { return false };
    let Some(ty) = UTType::typeWithFilenameExtension(&NSString::from_str(&ext.to_string_lossy())) else {
        return false;
    };
    ty.conformsToType(unsafe { UTTypeImage })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(names: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("reaview-test-{}-{}", std::process::id(), names.len()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in names {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        dir
    }

    fn names(paths: &[PathBuf]) -> Vec<String> {
        paths.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn lists_only_visible_images_in_finder_order() {
        let dir = folder(&["img10.heic", "img2.JPG", "img1.png", "notes.txt", ".hidden.png", "Photo.webp", "raw.CR2"]);
        let siblings = sibling_images(&dir.join("img1.png"));
        assert_eq!(names(&siblings), ["img1.png", "img2.JPG", "img10.heic", "Photo.webp", "raw.CR2"]);
    }

    #[test]
    fn steps_without_wrapping() {
        let dir = folder(&["a.png", "b.png", "c.png"]);
        let siblings = sibling_images(&dir.join("a.png"));
        let at = |name: &str, delta| neighbour(&siblings, &dir.join(name), delta).map(|p| names(&[p.clone()])[0].clone());
        assert_eq!(at("a.png", 1).as_deref(), Some("b.png"));
        assert_eq!(at("c.png", -1).as_deref(), Some("b.png"));
        assert_eq!(at("a.png", -1), None);
        assert_eq!(at("c.png", 1), None);
    }

    #[test]
    fn steps_from_where_a_deleted_file_used_to_be() {
        let dir = folder(&["a.png", "c.png", "e.png", "x.txt"]);
        let siblings = sibling_images(&dir.join("a.png"));
        let at = |name: &str, delta| neighbour(&siblings, &dir.join(name), delta).map(|p| names(&[p.clone()])[0].clone());
        assert_eq!(at("d.png", 1).as_deref(), Some("e.png"));
        assert_eq!(at("d.png", -1).as_deref(), Some("c.png"));
    }
}
