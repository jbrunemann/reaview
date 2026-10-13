//! Folder access under the App Sandbox.
//!
//! A sandboxed app opened on one file can read that file only, not its
//! siblings. The first time the user steps through a folder we can't list,
//! we ask for it with an Open panel (the "powerbox"), then keep the grant as a
//! security-scoped bookmark so later launches can list it without asking.
//! Unsandboxed (e.g. `cargo run`) every folder is listable and none of this
//! is visible.

use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
use objc2_foundation::{
    NSArray, NSData, NSString, NSURL, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions, NSUserDefaults,
};

const BOOKMARKS_KEY: &str = "FolderBookmarks";

/// Whether the folder can be read right now.
pub fn can_list(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok()
}

/// Start accessing every folder granted in earlier launches.
/// Bookmarks that no longer resolve are dropped.
pub fn restore() {
    let kept: Vec<Retained<NSData>> = stored()
        .into_iter()
        .filter(|data| resolve(data).is_some_and(|url| unsafe { url.startAccessingSecurityScopedResource() }))
        .collect();
    save(&kept);
}

/// Ask the user to grant read access to `dir`. Returns whether it is listable afterwards.
pub fn request(dir: &Path, mtm: MainThreadMarker) -> bool {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(false);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(false);
    panel.setDirectoryURL(NSURL::from_directory_path(dir).as_deref());
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "/".into());
    panel.setMessage(Some(&NSString::from_str(&format!(
        "Reaview needs access to the folder “{name}” to show the other images in it."
    ))));
    panel.setPrompt(Some(&NSString::from_str("Allow")));
    if panel.runModal() != NSModalResponseOK {
        return false;
    }
    let Some(url) = panel.URL() else { return false };
    if !unsafe { url.startAccessingSecurityScopedResource() } && !can_list(dir) {
        return false;
    }
    let options = NSURLBookmarkCreationOptions::WithSecurityScope
        | NSURLBookmarkCreationOptions::SecurityScopeAllowOnlyReadAccess;
    if let (Ok(data), Some(granted)) =
        (url.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(options, None, None), url.to_file_path())
    {
        // The new grant covers any stored folder inside it.
        let mut kept: Vec<Retained<NSData>> = stored()
            .into_iter()
            .filter(|old| !resolved_path(old).is_some_and(|p| p.starts_with(&granted)))
            .collect();
        kept.push(data);
        save(&kept);
    }
    can_list(dir)
}

fn stored() -> Vec<Retained<NSData>> {
    let defaults = NSUserDefaults::standardUserDefaults();
    let Some(array) = defaults.arrayForKey(&NSString::from_str(BOOKMARKS_KEY)) else { return Vec::new() };
    array.iter().filter_map(|obj| obj.downcast::<NSData>().ok()).collect()
}

fn save(bookmarks: &[Retained<NSData>]) {
    let array = NSArray::from_retained_slice(bookmarks);
    let value: &AnyObject = &array;
    unsafe {
        NSUserDefaults::standardUserDefaults().setObject_forKey(Some(value), &NSString::from_str(BOOKMARKS_KEY));
    }
}

fn resolve(data: &NSData) -> Option<Retained<NSURL>> {
    // A stale bookmark still resolves; it is refreshed when the folder is granted again.
    let mut stale = Bool::NO;
    unsafe {
        NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
            data,
            NSURLBookmarkResolutionOptions::WithSecurityScope | NSURLBookmarkResolutionOptions::WithoutUI,
            None,
            &mut stale,
        )
    }
    .ok()
}

fn resolved_path(data: &NSData) -> Option<PathBuf> {
    resolve(data).and_then(|url| url.to_file_path())
}
