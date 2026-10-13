# Reaview

A Preview.app-style image viewer for macOS, written in Rust on native AppKit via `objc2`.
Its defining feature: ← / → load the previous / next image in the same folder.

## Commands

```sh
cargo build                    # debug build, binary is target/debug/Reaview
cargo run -- path/to/image.jpg # no path → Open panel
cargo test                     # folder navigation + status-bar text tests
./scripts/bundle.sh            # release build → target/Reaview.app (needed for Finder "Open With")
swift scripts/make-icon.swift assets/AppIcon.png   # redraw the icon, then rebuild AppIcon.icns (see below)
```

Rebuilding `assets/AppIcon.icns` after changing the icon: the Swift script renders at 2× (2048 px),
so `sips -z 1024 1024 assets/AppIcon.png`, then make an `.iconset` with sips (16…512 plus @2x) and
`iconutil -c icns`. Both the PNG and the .icns are committed; the .icns is embedded with
`include_bytes!` and copied into the bundle.

## Layout

- `src/main.rs`: the app. `AppDelegate` (an `objc2` `define_class!`) is the app delegate, the window
  delegate and the target for every menu action. `Viewer` holds the window, scroll view, image view
  and the two status-bar labels. `CenteringClipView` centres images smaller than the viewport.
- `src/folder.rs`: lists images next to the current file (UTType conforms to `public.image`, hidden
  files skipped, Finder order via `localizedStandardCompare:`) and steps through them.
- `src/info.rs`: status-bar text: Finder "Kind", pixel size (largest image rep), file size.
- `scripts/bundle.sh`: builds `Reaview.app` with Info.plist (registers as a Viewer for `public.image`),
  ad-hoc signed with the hardened runtime and the App Sandbox (read-only user-selected files,
  app-scope bookmarks).
- `src/access.rs`: sandbox folder access. A sandboxed app can read only the file it was opened on,
  so the first ← / → in a folder it can't list shows an Open panel asking for that folder; the grant
  is stored as a security-scoped bookmark in user defaults and re-activated at launch.
- `assets/`: app icon.

## Behaviour and decisions

- Arrow keys are caught by a local `NSEvent` key-down monitor, only for the viewer window and only
  without modifiers. They take priority over scrolling when zoomed in.
- Navigation stops at either end with a beep (no wrap). The folder is re-read on every step; if the
  current file was deleted, stepping continues from where it used to sort.
- Decoding is `NSImage` (ImageIO), so every format Preview opens works.
- Zoom is `NSScrollView` magnification. "Sticky zoom to fit" (as in Preview): the image refits on
  window resize until the user zooms manually. Fit never enlarges past 100%.
- Shortcuts: ⌘0 actual size, ⌘9 fit, ⌘+ / ⌘− zoom, ⌘C copy image, ⌥⌘C copy path, ⌘O open.
  Right-click menu on the image (and scroll view): Copy Image, Copy Path.
- Window title is the file name, subtitle "n of N". The bottom status bar uses the window's content
  border; the zoom label updates from `NSViewBoundsDidChangeNotification` on the clip view, which
  also covers pinch-to-zoom.
- Unsandboxed (`cargo run`) every folder is listable, so the access prompt never appears. The
  bundled app is sandboxed: launch it with `open -a target/Reaview.app file`, not by running the
  binary inside it (the sandbox would block a path passed on the command line). Its defaults and
  bookmarks live in `~/Library/Containers/net.kaliber.reaview`.
- Command-line paths are made absolute with `std::path::absolute` (keeps symlink names).

## Working with objc2 (0.6 / AppKit bindings 0.3)

- Every AppKit/Foundation class used needs its Cargo feature in `Cargo.toml`
  (e.g. `NSPasteboard`, `NSTextField`). A "method not found" error usually means a missing feature.
- `NSImage` and other non-main-thread classes need `AllocAnyThread` in scope for `::alloc()`;
  `.retain()` needs `objc2::Message`.
- Some setters are `unsafe` (`setMenu`, `setTarget`, `setReleasedWhenClosed`, notification observers).
- New menu actions: add a `#[unsafe(method(name:))]` fn to the `AppDelegate` menu-actions impl and
  a `menu_item(...)` in `build_main_menu`.
- Look up exact signatures in `~/.cargo/registry/src/*/objc2-app-kit-0.3.2/src/generated/`.

## Testing limits

This environment cannot send keystrokes (osascript has no Accessibility access) or take screenshots
(no Screen Recording), so GUI behaviour must be checked by the user. Keep logic in testable modules
(`folder.rs`, `info.rs`) and smoke-test that the app launches.

## Reverse-engineering Preview.app with REA

Design choices came from REA (rea-agents MCP) static analysis of
`/System/Applications/Preview.app` with Hopper: `PVImageContainer` handles images,
`PVIKImageView2` (an ImageKit subclass) draws them, `PVWindowController` only navigates between
documents already open in a window, and `setStickyZoomToFit:` exists. Hopper takes ~3 minutes to
analyse Preview, which needs a local patch to REA's startup timeout; see the user's Claude memory
for the setup quirks.
