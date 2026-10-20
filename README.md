# Reaview

A fast, native macOS image viewer in the spirit of Preview.app, written in Rust.

![Reaview showing an image, with the folder position in the title and the status bar below](docs/screenshot.png)

- **← / →** step through every image in the same folder (Finder order)
- Opens anything macOS can decode: JPEG, PNG, HEIC, RAW, WebP, AVIF, GIF, TIFF, …
- Zoom to fit (⌘9), actual size (⌘0), zoom in/out (⌘+ / ⌘−), pinch to zoom
- Status bar with file type, pixel size, file size and zoom percentage
- Copy the image (⌘C) or its full path (⌥⌘C), also from the right-click menu
- **Reaview > Check for Updates…** asks GitHub for the latest release, only when you choose it

## Install

Download `Reaview-universal.zip` (Apple Silicon and Intel) from the
[latest release](../../releases/latest), unzip it and move `Reaview.app` to Applications.

The app is ad-hoc signed, not notarized, so the first launch needs a right-click, **Open**.
Or clear the quarantine flag: `xattr -dr com.apple.quarantine Reaview.app`.

## Listing from another app

← / → follow Finder name order, without hidden files, unless Reaview is started with options.
Another app can ask for `--sort name|size|date` (modified) with `--asc` or `--desc`, `--plain`
(names compared without numbers by value, "img10" before "img2") and `--hidden` (files starting
with a dot). Arguments only apply to a fresh process, so start a new instance:

```sh
open -n -a Reaview photo.jpg --args --sort date --desc --plain --hidden
```

## Make it the default image viewer

macOS stores the default app per file type, so there is no single switch for all images.
The bundle ID is `dev.reaview.ai`. Reaview registers as a viewer for `public.image`, so it shows
up in Finder's **Open With** menu.

**In Finder**, one type at a time: select an image, press ⌘I, choose Reaview under
**Open with:**, and click **Change All…**. Repeat for each format you use.

**With [`duti`](https://github.com/moretension/duti)**, all common types at once:

```sh
brew install duti
for t in \
  public.jpeg public.png public.heic public.heif public.avif \
  com.compuserve.gif public.tiff org.webmproject.webp \
  com.apple.icns com.microsoft.bmp com.microsoft.ico \
  public.jpeg-2000 public.jpeg-xl \
  public.camera-raw-image com.adobe.raw-image \
  com.adobe.photoshop-image com.truevision.tga-image \
  com.ilm.openexr-image com.apple.pict com.sgi.sgi-image public.radiance; do
  duti -s dev.reaview.ai $t all
done
duti -x jpg   # check: should list Reaview
```

If the app was just installed or replaced and `duti` doesn't see it, register it first:

```sh
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f /Applications/Reaview.app
```

Notes:

- SVG (`public.svg-image`) and PDF are left out on purpose. Add `public.svg-image` to the list if
  you want Reaview to open SVGs.
- `public.camera-raw-image` covers most RAW formats. If a RAW file still opens elsewhere, set it
  by extension: `duti -s dev.reaview.ai .cr2 all`.
- The app is sandboxed, so the first ← / → in a folder asks for access to that folder. The grant
  is remembered.

**Restore Preview**: run the same loop with Preview's bundle ID, `com.apple.Preview`
(for example `duti -s com.apple.Preview $t all`), and reset any extensions you set by hand, such as
`duti -s com.apple.Preview .cr2 all`. Or use **Change All…** in Finder again. To remove Reaview,
restore Preview first, then delete `/Applications/Reaview.app`.

## Build

```sh
cargo run -- path/to/image.jpg   # run from source
./scripts/bundle.sh              # build target/Reaview.app for this Mac
UNIVERSAL=1 ./scripts/bundle.sh  # arm64 + x86_64 app (needs both rustup targets)
```

Requires macOS 11+ and a Rust toolchain. For a universal build run
`rustup target add aarch64-apple-darwin x86_64-apple-darwin` once.

GitHub Actions (`.github/workflows/build.yml`) builds the universal app on every push and
pull request. Pushing a tag like `v0.1.0` also publishes `Reaview-universal.zip` as a release.
