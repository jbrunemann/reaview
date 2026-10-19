//! Finding the images that sit next to the current one in its folder.

use std::cmp::Ordering;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use objc2::rc::Retained;
use objc2_foundation::{NSComparisonResult, NSString};
use objc2_uniform_type_identifiers::{UTType, UTTypeImage};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortKey {
    /// Finder's name order.
    #[default]
    Name,
    Size,
    /// Modification date.
    Date,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Order {
    #[default]
    Asc,
    Desc,
}

/// Which images of a folder are listed and in what order. Another app that
/// opens a file here can ask for its own listing with `--sort name|size|date`,
/// `--asc` / `--desc`, `--plain` (names compared without numbers by value) and
/// `--hidden` (files starting with a dot included).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub key: SortKey,
    pub order: Order,
    pub plain: bool,
    pub hidden: bool,
}

impl Listing {
    /// Reads the options from command-line arguments (without the program
    /// name). `--sort=date` works too; unknown arguments and values are ignored.
    pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Self {
        let mut sort = Listing::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let value = match arg.to_str() {
                Some("--asc") => {
                    sort.order = Order::Asc;
                    continue;
                }
                Some("--desc") => {
                    sort.order = Order::Desc;
                    continue;
                }
                Some("--plain") => {
                    sort.plain = true;
                    continue;
                }
                Some("--hidden") => {
                    sort.hidden = true;
                    continue;
                }
                Some("--sort") => args.next(),
                Some(arg) => arg.strip_prefix("--sort=").map(OsString::from),
                None => None,
            };
            match value.as_deref().and_then(|v| v.to_str()) {
                Some("name") => sort.key = SortKey::Name,
                Some("size") => sort.key = SortKey::Size,
                Some("date") => sort.key = SortKey::Date,
                _ => {}
            }
        }
        sort
    }
}

struct Image {
    name: Retained<NSString>,
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

/// Every image in the folder containing `path`, in the given order (names the
/// way Finder sorts them; equal sizes or dates fall back to name order).
/// Hidden files are skipped unless asked for, and so is anything that is not an image.
/// The folder is re-read on every call so added or removed files show up.
pub fn sibling_images(path: &Path, sort: Listing) -> Vec<PathBuf> {
    let Some(dir) = path.parent() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut images: Vec<Image> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file() || t.is_symlink()))
        .map(|entry| entry.path())
        .filter(|path| (sort.hidden || !is_hidden(path)) && is_image(path))
        .map(|path| {
            // Symlinks count as the file they point to.
            let metadata = if sort.key == SortKey::Name { None } else { std::fs::metadata(&path).ok() };
            Image {
                name: file_name(&path),
                size: metadata.as_ref().map_or(0, |m| m.len()),
                modified: metadata.and_then(|m| m.modified().ok()).unwrap_or(UNIX_EPOCH),
                path,
            }
        })
        .collect();
    images.sort_by(|a, b| {
        let by_key = match sort.key {
            SortKey::Name => name_order(a, b, sort.plain),
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::Date => a.modified.cmp(&b.modified),
        };
        let by_key = if sort.order == Order::Desc { by_key.reverse() } else { by_key };
        by_key.then_with(|| name_order(a, b, sort.plain))
    });
    images.into_iter().map(|image| image.path).collect()
}

/// The image `delta` steps away from `current`, or `None` at either end.
/// If `current` has been deleted, the nearest image that still exists in
/// that direction in the `previous` listing is the target (a deleted file has
/// no size or date left to sort by, so its place is taken from the old list).
pub fn neighbour<'a>(siblings: &'a [PathBuf], current: &Path, delta: isize, previous: &[PathBuf]) -> Option<&'a PathBuf> {
    if let Some(index) = siblings.iter().position(|p| p == current) {
        return usize::try_from(index as isize + delta).ok().and_then(|index| siblings.get(index));
    }
    let at = previous.iter().position(|p| p == current)?;
    let mut candidates: Box<dyn Iterator<Item = &PathBuf>> =
        if delta > 0 { Box::new(previous[at + 1..].iter()) } else { Box::new(previous[..at].iter().rev()) };
    candidates.find_map(|candidate| siblings.iter().find(|p| *p == candidate))
}

fn file_name(path: &Path) -> Retained<NSString> {
    NSString::from_str(&path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default())
}

/// The order of two images by name. Names that compare equal (differing only in
/// case) are told apart by their characters, which keeps the order total.
/// `plain` ignores digit values: "img10" before "img2".
fn name_order(a: &Image, b: &Image, plain: bool) -> Ordering {
    let by_name = if plain { plain_order(&a.name, &b.name) } else { finder_order(&a.name, &b.name) };
    by_name.then_with(|| a.path.file_name().cmp(&b.path.file_name()))
}

fn plain_order(a: &NSString, b: &NSString) -> Ordering {
    match a.localizedCaseInsensitiveCompare(b) {
        NSComparisonResult::Ascending => Ordering::Less,
        NSComparisonResult::Descending => Ordering::Greater,
        _ => Ordering::Equal,
    }
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
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    use std::time::Duration;

    /// A fresh folder holding empty files with these names.
    fn folder(names: &[&str]) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("reaview-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Relaxed)));
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

    fn sort(key: SortKey, order: Order) -> Listing {
        Listing { key, order, ..Listing::default() }
    }

    fn args(args: &[&str]) -> Listing {
        Listing::from_args(args.iter().map(OsString::from))
    }

    #[test]
    fn lists_only_visible_images_in_finder_order() {
        let dir = folder(&["img10.heic", "img2.JPG", "img1.png", "notes.txt", ".hidden.png", "Photo.webp", "raw.CR2"]);
        let siblings = sibling_images(&dir.join("img1.png"), Listing::default());
        assert_eq!(names(&siblings), ["img1.png", "img2.JPG", "img10.heic", "Photo.webp", "raw.CR2"]);
    }

    #[test]
    fn hidden_files_are_listed_when_asked() {
        let dir = folder(&["a.png", ".b.png", "notes.txt"]);
        let at = |hidden| names(&sibling_images(&dir.join("a.png"), Listing { hidden, ..Listing::default() }));
        assert_eq!(at(false), ["a.png"]);
        assert_eq!(at(true), [".b.png", "a.png"]);
    }

    #[test]
    fn plain_order_ignores_digit_values() {
        let dir = folder(&["img10.png", "img2.png", "img1.png", "Img3.png"]);
        let at = |plain| names(&sibling_images(&dir.join("img1.png"), Listing { plain, ..Listing::default() }));
        assert_eq!(at(false), ["img1.png", "img2.png", "Img3.png", "img10.png"]);
        assert_eq!(at(true), ["img1.png", "img10.png", "img2.png", "Img3.png"]);
    }

    #[test]
    fn names_differing_only_in_case_are_never_equal() {
        // Not through a folder: a case-insensitive volume can't hold both files.
        let image = |name: &str| Image {
            name: NSString::from_str(name),
            path: PathBuf::from(name),
            size: 0,
            modified: UNIX_EPOCH,
        };
        let (upper, lower) = (image("A.png"), image("a.png"));
        for plain in [false, true] {
            let order = name_order(&upper, &lower, plain);
            assert_ne!(order, Ordering::Equal);
            assert_eq!(name_order(&lower, &upper, plain), order.reverse());
        }
    }

    #[test]
    fn name_order_can_be_reversed() {
        let dir = folder(&["img10.png", "img2.png", "img1.png"]);
        let siblings = sibling_images(&dir.join("img1.png"), sort(SortKey::Name, Order::Desc));
        assert_eq!(names(&siblings), ["img10.png", "img2.png", "img1.png"]);
    }

    #[test]
    fn sorts_by_size_with_name_order_for_ties() {
        let dir = folder(&[]);
        for (name, len) in [("a.png", 30), ("b.png", 10), ("c.png", 20), ("d.png", 10)] {
            std::fs::write(dir.join(name), vec![0; len]).unwrap();
        }
        let at = |order| names(&sibling_images(&dir.join("a.png"), sort(SortKey::Size, order)));
        assert_eq!(at(Order::Asc), ["b.png", "d.png", "c.png", "a.png"]);
        assert_eq!(at(Order::Desc), ["a.png", "c.png", "b.png", "d.png"]);
    }

    #[test]
    fn sorts_by_modification_date() {
        let dir = folder(&["a.png", "b.png", "c.png"]);
        for (name, age) in [("a.png", 10), ("b.png", 30), ("c.png", 20)] {
            let file = std::fs::File::options().write(true).open(dir.join(name)).unwrap();
            file.set_modified(UNIX_EPOCH + Duration::from_secs(1_000_000 - age)).unwrap();
        }
        let at = |order| names(&sibling_images(&dir.join("a.png"), sort(SortKey::Date, order)));
        assert_eq!(at(Order::Asc), ["b.png", "c.png", "a.png"]);
        assert_eq!(at(Order::Desc), ["a.png", "c.png", "b.png"]);
    }

    #[test]
    fn steps_without_wrapping() {
        let dir = folder(&["a.png", "b.png", "c.png"]);
        let siblings = sibling_images(&dir.join("a.png"), Listing::default());
        let at = |name: &str, delta| neighbour(&siblings, &dir.join(name), delta, &siblings).map(|p| names(&[p.clone()])[0].clone());
        assert_eq!(at("a.png", 1).as_deref(), Some("b.png"));
        assert_eq!(at("c.png", -1).as_deref(), Some("b.png"));
        assert_eq!(at("a.png", -1), None);
        assert_eq!(at("c.png", 1), None);
    }

    #[test]
    fn steps_from_where_a_deleted_file_used_to_be() {
        let dir = folder(&["a.png", "c.png", "d.png", "e.png", "x.txt"]);
        let sort = sort(SortKey::Size, Order::Desc);
        let previous = sibling_images(&dir.join("a.png"), sort);
        std::fs::remove_file(dir.join("d.png")).unwrap();
        let siblings = sibling_images(&dir.join("a.png"), sort);
        let at = |delta| neighbour(&siblings, &dir.join("d.png"), delta, &previous).map(|p| names(&[p.clone()])[0].clone());
        // All sizes are equal, so the order is by name: a, c, d, e.
        assert_eq!(at(1).as_deref(), Some("e.png"));
        assert_eq!(at(-1).as_deref(), Some("c.png"));
    }

    #[test]
    fn deleted_file_at_the_end_has_no_neighbour_there() {
        let dir = folder(&["a.png", "b.png"]);
        let previous = sibling_images(&dir.join("a.png"), Listing::default());
        std::fs::remove_file(dir.join("b.png")).unwrap();
        let siblings = sibling_images(&dir.join("a.png"), Listing::default());
        assert_eq!(neighbour(&siblings, &dir.join("b.png"), 1, &previous), None);
        assert_eq!(neighbour(&siblings, &dir.join("b.png"), -1, &previous), Some(&dir.join("a.png")));
    }

    #[test]
    fn reads_sort_from_arguments() {
        assert_eq!(args(&[]), Listing::default());
        assert_eq!(args(&["--sort", "date", "--desc"]), sort(SortKey::Date, Order::Desc));
        assert_eq!(args(&["--sort=size"]), sort(SortKey::Size, Order::Asc));
        assert_eq!(args(&["--desc", "--sort", "name", "--asc"]), sort(SortKey::Name, Order::Asc));
        assert_eq!(args(&["--plain", "--hidden"]), Listing { plain: true, hidden: true, ..Listing::default() });
        // Unknown keys and stray arguments change nothing.
        assert_eq!(args(&["--sort", "kind", "-psn_0_1234", "--sort"]), Listing::default());
    }
}
