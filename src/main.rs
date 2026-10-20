//! Reaview: a minimal Preview.app-style image viewer.
//!
//! Mirrors the behaviour observed in Preview.app via REA static analysis:
//! images are decoded through ImageIO (NSImage), the view keeps a "sticky"
//! zoom-to-fit while the window resizes, and Actual Size / Zoom In / Zoom Out /
//! Zoom to Fit use Preview's key equivalents. Unlike Preview, the left and
//! right arrow keys load the previous and next image in the same folder.

mod access;
mod folder;
mod info;
mod update;

use std::cell::{Cell, OnceCell, RefCell};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSWorkspace, NSApplication, NSApplicationActivationPolicy, NSAutoresizingMaskOptions, NSApplicationDelegate, NSBackingStoreType,
    NSBeep, NSClipView, NSColor, NSEvent, NSEventMask, NSEventModifierFlags, NSFont,
    NSFontWeightRegular, NSImage, NSLineBreakMode, NSTextField, NSView, NSViewBoundsDidChangeNotification,
    NSImageScaling, NSImageView, NSMenu, NSMenuItem, NSModalResponseOK, NSOpenPanel,
    NSPasteboard, NSPasteboardTypeString, NSPasteboardWriting, NSScrollView,
    NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSData, NSNotification, NSNotificationCenter, NSObject, NSObjectProtocol, NSPoint,
    NSRect, NSRectEdge, NSSize, NSString, NSURL,
};
use objc2_uniform_type_identifiers::UTTypeImage;

const APP_NAME: &str = "Reaview";
const APP_ICON: &[u8] = include_bytes!("../assets/AppIcon.icns");

const KEY_LEFT_ARROW: u16 = 123;
const KEY_RIGHT_ARROW: u16 = 124;
const MIN_MAGNIFICATION: f64 = 0.02;
const MAX_MAGNIFICATION: f64 = 32.0;
const ZOOM_STEP: f64 = 1.25;
const STATUS_BAR_HEIGHT: f64 = 24.0;

// A clip view that centres the document when it is smaller than the viewport,
// the way Preview shows a small image in the middle of the window.
define_class!(
    #[unsafe(super(NSClipView, objc2_app_kit::NSView, objc2_app_kit::NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RVCenteringClipView"]
    struct CenteringClipView;

    impl CenteringClipView {
        #[unsafe(method(constrainBoundsRect:))]
        fn constrain_bounds_rect(&self, proposed: NSRect) -> NSRect {
            let mut rect: NSRect = unsafe { msg_send![super(self), constrainBoundsRect: proposed] };
            if let Some(document) = self.documentView() {
                let frame = document.frame();
                if rect.size.width > frame.size.width {
                    rect.origin.x = (frame.size.width - rect.size.width) / 2.0;
                }
                if rect.size.height > frame.size.height {
                    rect.origin.y = (frame.size.height - rect.size.height) / 2.0;
                }
            }
            rect
        }
    }
);

impl CenteringClipView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

struct Viewer {
    window: Retained<NSWindow>,
    scroll_view: Retained<NSScrollView>,
    image_view: Retained<NSImageView>,
    info_label: Retained<NSTextField>,
    zoom_label: Retained<NSTextField>,
}

#[derive(Default)]
struct AppState {
    viewer: OnceCell<Viewer>,
    current: RefCell<Option<PathBuf>>,
    // Which images are listed, and in what order (`--sort`, `--asc`, `--desc`,
    // `--plain`, `--hidden` on the command line), and the listing it gave when
    // the current image was loaded.
    listing: Cell<folder::Listing>,
    siblings: RefCell<Vec<PathBuf>>,
    image_size: Cell<NSSize>,
    // Preview's "sticky zoom to fit": stays fitted while the window resizes,
    // until the user zooms explicitly.
    sticky_fit: Cell<bool>,
    // An update check is in flight; the menu item does nothing meanwhile.
    checking_updates: Cell<bool>,
    key_monitor: OnceCell<Retained<AnyObject>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RVAppDelegate"]
    #[ivars = AppState]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            self.install_key_monitor();            access::restore();
            let path = std::env::args_os().skip(1).map(PathBuf::from).find(|p| p.is_file());
            match path {
                Some(path) => self.open_path(&path),
                // Finder "Open With" delivers files through application:openURLs:.
                None if self.ivars().current.borrow().is_none() => self.run_open_panel(true),
                None => {}
            }
            let app = NSApplication::sharedApplication(self.mtm());
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
        }

        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _application: &NSApplication, urls: &NSArray<NSURL>) {
            if let Some(path) = urls.iter().find_map(|url| url.to_file_path()) {
                self.open_path(&path);
            }
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window_closed(&self, _application: &NSApplication) -> bool {
            true
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _notification: &NSNotification) {
            if self.ivars().sticky_fit.get() {
                self.zoom_to_fit();
            }
        }
    }

    // Menu actions.
    impl AppDelegate {
        #[unsafe(method(openDocument:))]
        fn open_document(&self, _sender: Option<&AnyObject>) {
            self.run_open_panel(false);
        }

        #[unsafe(method(checkForUpdates:))]
        fn check_for_updates(&self, _sender: Option<&AnyObject>) {
            if self.ivars().checking_updates.replace(true) {
                return;
            }
            let mtm = self.mtm();
            let this = self.retain();
            update::check(&update::source(), env!("CARGO_PKG_VERSION"), move |result| {
                this.ivars().checking_updates.set(false);
                show_update_result(mtm, result);
            });
        }

        #[unsafe(method(goToPreviousImage:))]
        fn go_to_previous_image(&self, _sender: Option<&AnyObject>) {
            self.step(-1);
        }

        #[unsafe(method(goToNextImage:))]
        fn go_to_next_image(&self, _sender: Option<&AnyObject>) {
            self.step(1);
        }

        #[unsafe(method(clipBoundsDidChange:))]
        fn clip_bounds_did_change(&self, _notification: &NSNotification) {
            // Fires for pinch-to-zoom as well as our own zoom commands.
            self.update_zoom_label();
        }

        #[unsafe(method(copyImage:))]
        fn copy_image(&self, _sender: Option<&AnyObject>) {
            let Some(image) = self.ivars().viewer.get().and_then(|v| v.image_view.image()) else {
                NSBeep();
                return;
            };
            let pasteboard = NSPasteboard::generalPasteboard();
            pasteboard.clearContents();
            let item: Retained<ProtocolObject<dyn NSPasteboardWriting>> = ProtocolObject::from_retained(image);
            pasteboard.writeObjects(&NSArray::from_retained_slice(&[item]));
        }

        #[unsafe(method(copyPath:))]
        fn copy_path(&self, _sender: Option<&AnyObject>) {
            let Some(path) = self.ivars().current.borrow().clone() else {
                NSBeep();
                return;
            };
            let pasteboard = NSPasteboard::generalPasteboard();
            pasteboard.clearContents();
            pasteboard.setString_forType(
                &NSString::from_str(&path.to_string_lossy()),
                unsafe { NSPasteboardTypeString },
            );
        }

        #[unsafe(method(doActualSize:))]
        fn do_actual_size(&self, _sender: Option<&AnyObject>) {
            self.ivars().sticky_fit.set(false);
            self.set_magnification(1.0);
        }

        #[unsafe(method(doZoomToFit:))]
        fn do_zoom_to_fit(&self, _sender: Option<&AnyObject>) {
            self.ivars().sticky_fit.set(true);
            self.zoom_to_fit();
        }

        #[unsafe(method(doZoomIn:))]
        fn do_zoom_in(&self, _sender: Option<&AnyObject>) {
            self.zoom_by(ZOOM_STEP);
        }

        #[unsafe(method(doZoomOut:))]
        fn do_zoom_out(&self, _sender: Option<&AnyObject>) {
            self.zoom_by(1.0 / ZOOM_STEP);
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker, listing: folder::Listing) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AppState { listing: Cell::new(listing), ..AppState::default() });
        unsafe { msg_send![super(this), init] }
    }

    fn viewer(&self) -> &Viewer {
        self.ivars().viewer.get_or_init(|| self.make_viewer())
    }

    fn make_viewer(&self) -> Viewer {
        let mtm = self.mtm();
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(900.0, 650.0));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setMinSize(NSSize::new(240.0, 160.0));
        window.setDelegate(Some(ProtocolObject::from_ref(self)));

        let scroll_frame = NSRect::new(
            NSPoint::new(0.0, STATUS_BAR_HEIGHT),
            NSSize::new(frame.size.width, frame.size.height - STATUS_BAR_HEIGHT),
        );
        let scroll_view = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), scroll_frame);
        scroll_view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        scroll_view.setContentView(&CenteringClipView::new(mtm));
        scroll_view.setHasVerticalScroller(true);
        scroll_view.setHasHorizontalScroller(true);
        scroll_view.setAutohidesScrollers(true);
        scroll_view.setAllowsMagnification(true);
        scroll_view.setMinMagnification(MIN_MAGNIFICATION);
        scroll_view.setMaxMagnification(MAX_MAGNIFICATION);
        scroll_view.setDrawsBackground(true);
        scroll_view.setBackgroundColor(&NSColor::underPageBackgroundColor());

        let image_view = NSImageView::initWithFrame(NSImageView::alloc(mtm), NSRect::ZERO);
        image_view.setImageScaling(NSImageScaling::ScaleAxesIndependently);
        image_view.setAnimates(true);
        scroll_view.setDocumentView(Some(&image_view));

        // Right-click menu, on the scroll view too so clicks beside a small
        // image still get it.
        let context_menu = NSMenu::new(mtm);
        let none = NSEventModifierFlags::empty();
        for item in [
            menu_item(mtm, "Copy Image", Some(sel!(copyImage:)), "", none),
            menu_item(mtm, "Copy Path", Some(sel!(copyPath:)), "", none),
        ] {
            unsafe { item.setTarget(Some(self)) };
            context_menu.addItem(&item);
        }
        unsafe { image_view.setMenu(Some(&context_menu)) };
        unsafe { scroll_view.setMenu(Some(&context_menu)) };

        // Status bar: file details on the left, zoom on the right, drawn on the
        // window's native bottom border like Finder's status bar.
        window.setContentBorderThickness_forEdge(STATUS_BAR_HEIGHT, NSRectEdge::MinY);
        window.setAutorecalculatesContentBorderThickness_forEdge(false, NSRectEdge::MinY);
        let zoom_width = 60.0;
        let info_label = status_label(
            mtm,
            NSRect::new(NSPoint::new(10.0, 4.0), NSSize::new(frame.size.width - zoom_width - 30.0, 16.0)),
            NSAutoresizingMaskOptions::ViewWidthSizable,
        );
        let zoom_label = status_label(
            mtm,
            NSRect::new(NSPoint::new(frame.size.width - zoom_width - 10.0, 4.0), NSSize::new(zoom_width, 16.0)),
            NSAutoresizingMaskOptions::ViewMinXMargin,
        );
        zoom_label.setAlignment(objc2_app_kit::NSTextAlignment::Right);

        let content = NSView::initWithFrame(NSView::alloc(mtm), frame);
        content.addSubview(&scroll_view);
        content.addSubview(&info_label);
        content.addSubview(&zoom_label);
        window.setContentView(Some(&content));

        let clip = scroll_view.contentView();
        clip.setPostsBoundsChangedNotifications(true);
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                self,
                sel!(clipBoundsDidChange:),
                Some(NSViewBoundsDidChangeNotification),
                Some(&clip),
            );
        }
        Viewer { window, scroll_view, image_view, info_label, zoom_label }
    }

    fn install_key_monitor(&self) {
        let delegate: Retained<Self> = self.retain();
        let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let event_ref = unsafe { event.as_ref() };
            let ours = match (event_ref.window(delegate.mtm()), delegate.ivars().viewer.get()) {
                (Some(window), Some(viewer)) => window == viewer.window,
                _ => false,
            };
            let modifiers = event_ref.modifierFlags()
                & (NSEventModifierFlags::Command
                    | NSEventModifierFlags::Option
                    | NSEventModifierFlags::Control
                    | NSEventModifierFlags::Shift);
            if ours && modifiers.is_empty() {
                match event_ref.keyCode() {
                    KEY_LEFT_ARROW => {
                        delegate.step(-1);
                        return std::ptr::null_mut();
                    }
                    KEY_RIGHT_ARROW => {
                        delegate.step(1);
                        return std::ptr::null_mut();
                    }
                    _ => {}
                }
            }
            event.as_ptr()
        });
        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block)
        };
        if let Some(monitor) = monitor {
            let _ = self.ivars().key_monitor.set(monitor);
        }
    }

    fn run_open_panel(&self, terminate_on_cancel: bool) {
        let mtm = self.mtm();
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(false);
        let image_type = unsafe { UTTypeImage }.retain();
        panel.setAllowedContentTypes(&NSArray::from_retained_slice(&[image_type]));
        if panel.runModal() == NSModalResponseOK {
            if let Some(path) = panel.URLs().iter().find_map(|url| url.to_file_path()) {
                self.open_path(&path);
                return;
            }
        }
        if terminate_on_cancel && self.ivars().current.borrow().is_none() {
            NSApplication::sharedApplication(mtm).terminate(None);
        }
    }

    /// Open `path` in the viewer window, sizing the window to the image the
    /// first time it is shown.
    fn open_path(&self, path: &Path) {
        // Command-line paths may be relative; keep symlink names (no canonicalize).
        let Ok(path) = std::path::absolute(path) else { return };
        let path = path.as_path();
        let first_open = self.ivars().viewer.get().is_none();
        if !self.load(path) {
            NSBeep();
            return;
        }
        let viewer = self.viewer();
        if first_open {
            self.size_window_to_image();
            viewer.window.center();
        }
        self.ivars().sticky_fit.set(true);
        self.zoom_to_fit();
        viewer.window.makeKeyAndOrderFront(None);
    }

    /// Move `delta` images through the current folder, in the chosen order.
    fn step(&self, delta: isize) {
        let Some(current) = self.ivars().current.borrow().clone() else { return };
        // Sandboxed, only the opened file is readable until the user grants its folder.
        if let Some(dir) = current.parent() {
            if !access::can_list(dir) && !access::request(dir, self.mtm()) {
                NSBeep();
                return;
            }
        }
        let siblings = folder::sibling_images(&current, self.ivars().listing.get());
        let previous = self.ivars().siblings.borrow().clone();
        let Some(target) = folder::neighbour(&siblings, &current, delta, &previous) else {
            NSBeep();
            return;
        };
        if self.load(target) {
            self.ivars().sticky_fit.set(true);
            self.zoom_to_fit();
        } else {
            NSBeep();
        }
    }

    fn load(&self, path: &Path) -> bool {
        let url = NSURL::from_file_path(path).expect("absolute file path");
        let Some(image) = NSImage::initWithContentsOfURL(NSImage::alloc(), &url) else {
            return false;
        };
        let viewer = self.viewer();
        let size = image.size();
        viewer.image_view.setImage(Some(&image));
        viewer.info_label.setStringValue(&NSString::from_str(&info::describe(path, &image)));
        viewer.image_view.setFrameSize(size);
        self.ivars().image_size.set(size);

        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        viewer.window.setTitle(&NSString::from_str(&name));
        viewer.window.setRepresentedURL(Some(&url));
        let siblings = folder::sibling_images(path, self.ivars().listing.get());
        let subtitle = match siblings.iter().position(|p| p == path) {
            Some(index) => format!("{} of {}", index + 1, siblings.len()),
            None => String::new(),
        };
        viewer.window.setSubtitle(&NSString::from_str(&subtitle));
        *self.ivars().siblings.borrow_mut() = siblings;

        *self.ivars().current.borrow_mut() = Some(path.to_path_buf());
        true
    }

    fn size_window_to_image(&self) {
        let viewer = self.viewer();
        let Some(screen) = viewer.window.screen().or_else(|| NSScreenMain(self.mtm())) else {
            return;
        };
        let visible = screen.visibleFrame();
        let image = self.ivars().image_size.get();
        let max_width = visible.size.width * 0.9;
        let max_height = visible.size.height * 0.9 - STATUS_BAR_HEIGHT;
        let scale = (max_width / image.width).min(max_height / image.height).min(1.0);
        let content = NSSize::new(
            (image.width * scale).max(240.0).round(),
            (image.height * scale).max(160.0).round() + STATUS_BAR_HEIGHT,
        );
        viewer.window.setContentSize(content);
    }

    fn zoom_to_fit(&self) {
        let Some(viewer) = self.ivars().viewer.get() else { return };
        let image = self.ivars().image_size.get();
        if image.width <= 0.0 || image.height <= 0.0 {
            return;
        }
        let available = viewer.scroll_view.contentSize();
        // Fit large images to the window, but never enlarge a small image
        // past its actual size.
        let scale = (available.width / image.width)
            .min(available.height / image.height)
            .min(1.0);
        self.set_magnification(scale);
    }

    fn zoom_by(&self, factor: f64) {
        let Some(viewer) = self.ivars().viewer.get() else { return };
        self.ivars().sticky_fit.set(false);
        self.set_magnification(viewer.scroll_view.magnification() * factor);
    }

    fn set_magnification(&self, magnification: f64) {
        let Some(viewer) = self.ivars().viewer.get() else { return };
        let magnification = magnification.clamp(MIN_MAGNIFICATION, MAX_MAGNIFICATION);
        let clip = viewer.scroll_view.contentView();
        let visible = clip.bounds();
        let centre = NSPoint::new(
            visible.origin.x + visible.size.width / 2.0,
            visible.origin.y + visible.size.height / 2.0,
        );
        viewer.scroll_view.setMagnification_centeredAtPoint(magnification, centre);
        // Re-run the centring constraint after the magnification change.
        let bounds = clip.bounds();
        clip.setBoundsOrigin(clip.constrainBoundsRect(bounds).origin);
        self.update_zoom_label();
    }

    fn update_zoom_label(&self) {
        let Some(viewer) = self.ivars().viewer.get() else { return };
        let percent = (viewer.scroll_view.magnification() * 100.0).round();
        viewer.zoom_label.setStringValue(&NSString::from_str(&format!("{percent}%")));
    }
}

#[allow(non_snake_case)]
fn NSScreenMain(mtm: MainThreadMarker) -> Option<Retained<objc2_app_kit::NSScreen>> {
    objc2_app_kit::NSScreen::mainScreen(mtm)
}

/// Tells the user what "Check for Updates…" found; offers the download page when there is a newer release.
fn show_update_result(mtm: MainThreadMarker, result: Result<update::Status, update::Error>) {
    let version = env!("CARGO_PKG_VERSION");
    let alert = NSAlert::new(mtm);
    let mut page = None;
    match result {
        Ok(update::Status::Available(release)) => {
            alert.setMessageText(&NSString::from_str(&format!("Reaview {} is available", release.version)));
            alert.setInformativeText(&NSString::from_str(&format!("You have version {version}.")));
            page = release.page;
        }
        Ok(update::Status::UpToDate { .. }) => {
            alert.setMessageText(&NSString::from_str("Reaview is up to date"));
            alert.setInformativeText(&NSString::from_str(&format!("You have the latest version, {version}.")));
        }
        Err(error) => {
            alert.setMessageText(&NSString::from_str("Couldn’t check for updates"));
            alert.setInformativeText(&NSString::from_str(&format!("{}.", error.to_string().trim_end_matches('.'))));
        }
    }
    alert.addButtonWithTitle(&NSString::from_str(if page.is_some() { "Open Download Page" } else { "OK" }));
    if page.is_some() {
        alert.addButtonWithTitle(&NSString::from_str("Not Now"));
    }
    if alert.runModal() == NSAlertFirstButtonReturn {
        if let Some(url) = page.and_then(|p| NSURL::URLWithString(&NSString::from_str(&p))) {
            NSWorkspace::sharedWorkspace().openURL(&url);
        }
    }
}

fn status_label(
    mtm: MainThreadMarker,
    frame: NSRect,
    mask: NSAutoresizingMaskOptions,
) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::new(), mtm);
    label.setFrame(frame);
    label.setAutoresizingMask(mask | NSAutoresizingMaskOptions::ViewMaxYMargin);
    // Monospaced digits so the zoom percentage doesn't jitter while pinching.
    let font = NSFont::monospacedDigitSystemFontOfSize_weight(NSFont::smallSystemFontSize(), unsafe {
        NSFontWeightRegular
    });
    label.setFont(Some(&font));
    label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    label
}

fn menu_item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    key: &str,
    modifiers: NSEventModifierFlags,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(key),
        )
    };
    item.setKeyEquivalentModifierMask(modifiers);
    item
}

fn submenu(mtm: MainThreadMarker, title: &str, items: &[Retained<NSMenuItem>]) -> Retained<NSMenuItem> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
    for item in items {
        menu.addItem(item);
    }
    let holder = menu_item(mtm, title, None, "", NSEventModifierFlags::empty());
    holder.setSubmenu(Some(&menu));
    holder
}

fn build_main_menu(mtm: MainThreadMarker) -> Retained<NSMenu> {
    let cmd = NSEventModifierFlags::Command;
    let none = NSEventModifierFlags::empty();
    let left = char::from_u32(0xF702).unwrap().to_string();
    let right = char::from_u32(0xF703).unwrap().to_string();

    let main = NSMenu::new(mtm);
    main.addItem(&submenu(mtm, APP_NAME, &[
        menu_item(mtm, &format!("About {APP_NAME}"), Some(sel!(orderFrontStandardAboutPanel:)), "", none),
        menu_item(mtm, "Check for Updates…", Some(sel!(checkForUpdates:)), "", none),
        NSMenuItem::separatorItem(mtm),
        menu_item(mtm, &format!("Hide {APP_NAME}"), Some(sel!(hide:)), "h", cmd),
        NSMenuItem::separatorItem(mtm),
        menu_item(mtm, &format!("Quit {APP_NAME}"), Some(sel!(terminate:)), "q", cmd),
    ]));
    main.addItem(&submenu(mtm, "File", &[
        menu_item(mtm, "Open…", Some(sel!(openDocument:)), "o", cmd),
        menu_item(mtm, "Close", Some(sel!(performClose:)), "w", cmd),
    ]));
    main.addItem(&submenu(mtm, "Edit", &[
        menu_item(mtm, "Copy Image", Some(sel!(copyImage:)), "c", cmd),
        // Same shortcut as Finder's "Copy as Pathname".
        menu_item(mtm, "Copy Path", Some(sel!(copyPath:)), "c", cmd | NSEventModifierFlags::Option),
    ]));
    main.addItem(&submenu(mtm, "View", &[
        menu_item(mtm, "Actual Size", Some(sel!(doActualSize:)), "0", cmd),
        menu_item(mtm, "Zoom to Fit", Some(sel!(doZoomToFit:)), "9", cmd),
        menu_item(mtm, "Zoom In", Some(sel!(doZoomIn:)), "+", cmd),
        menu_item(mtm, "Zoom Out", Some(sel!(doZoomOut:)), "-", cmd),
    ]));
    main.addItem(&submenu(mtm, "Go", &[
        menu_item(mtm, "Previous Image", Some(sel!(goToPreviousImage:)), &left, none),
        menu_item(mtm, "Next Image", Some(sel!(goToNextImage:)), &right, none),
    ]));
    main.addItem(&submenu(mtm, "Window", &[
        menu_item(mtm, "Minimize", Some(sel!(performMiniaturize:)), "m", cmd),
        menu_item(mtm, "Zoom", Some(sel!(performZoom:)), "", none),
    ]));
    main
}

fn main() {
    let mtm = MainThreadMarker::new().expect("must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    app.setMainMenu(Some(&build_main_menu(mtm)));
    // Also covers `cargo run`, where there is no bundle to take the icon from.
    if let Some(icon) = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(APP_ICON)) {
        unsafe { app.setApplicationIconImage(Some(&icon)) };
    }

    let delegate = AppDelegate::new(mtm, folder::Listing::from_args(std::env::args_os().skip(1)));
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}
